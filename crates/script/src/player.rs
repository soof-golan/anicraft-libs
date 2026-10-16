//! The preview runtime: plays a document's main timeline, runs frame scripts as the playhead enters their frames, and
//! delivers events to the handlers the scripts registered.
//!
//! What is supported, exactly:
//!
//! - **Timeline control**: `stop()`, `play()`, `gotoAndPlay(frame | label [, scene])`, `gotoAndStop(...)`,
//!   `nextFrame()`, `prevFrame()`, `nextScene()`, `prevScene()`, as bare calls, on `this`, on `root` / `_root` /
//!   `MovieClip(root)`, and on named child clips (a child clip's own playhead is tracked and readable, but the preview
//!   renderer keeps playing child clips with their parent).
//! - **Reading state**: `currentFrame`, `totalFrames`, `currentLabel` (and the AS2 `_currentframe`, `_totalframes`).
//! - **Events**: `target.addEventListener(type, handler)` and `removeEventListener` for `MouseEvent.CLICK`,
//!   `MOUSE_DOWN`, `MOUSE_UP`, `MOUSE_OVER`, `MOUSE_OUT`, `ROLL_OVER`, `ROLL_OUT`, `DOUBLE_CLICK` and
//!   `Event.ENTER_FRAME`; the `ActionScript` 2 forms `target.onPress`, `onRelease`, `onRollOver`, `onRollOut`,
//!   `onEnterFrame = function () {...}`.
//! - **Language**: `var` with number, string and boolean values, assignment, arithmetic, comparison and logic,
//!   `if`/`else`, function declarations and literals (handlers), `return`, and `trace`. Frame scripts share the
//!   variables of the main timeline.
//! - **Not supported** (reported by the parser, skipped at run time): loops, `switch`, `try`, classes, `new`, arrays
//!   and objects, bracket access, the conditional operator, and every other class or function of the Flash API.
//!   Scripts inside movie clip and button symbols are exported to SWF but not run by the preview.

use std::collections::HashMap;

use anim_core::num::round_f64_to_u32;
use anim_scene::element::Element;
use anim_scene::timeline::Timeline;
use anim_scene::{Document, Symbol};
use serde::{Deserialize, Serialize};

use crate::ast::{BinOp, Expr, Func, Script, Stmt, UnOp};
use crate::parse::parse;
use crate::value::{Value, loose_equals, strict_equals};

/// Most statements and expressions one external call may evaluate before it is stopped.
const STEP_LIMIT: u32 = 200_000;
/// Deepest function call nesting.
const CALL_DEPTH_LIMIT: u32 = 32;
/// How many times one tick may chain from a script into the next frame's scripts.
const CHAIN_LIMIT: u32 = 64;
/// Output lines kept.
const OUTPUT_LIMIT: usize = 1000;

/// Which `ActionScript` a document uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Version {
    As2,
    #[default]
    As3,
}

/// The `ActionScript` version of the document's publish settings (AS3 when they say nothing).
#[must_use]
pub fn script_version(doc: &Document) -> Version {
    let Some(xml) = doc.publish_settings_xml.as_deref() else { return Version::default() };
    let Some(at) = xml.find("<ActionScriptVersion>") else { return Version::default() };
    let rest = xml.get(at.saturating_add("<ActionScriptVersion>".len())..).unwrap_or("");
    match rest.trim_start().chars().next() {
        Some('2' | '1') => Version::As2,
        _ => Version::As3,
    }
}

/// The name of the symbol instance at `(layer, element)` of the keyframe covering `frame` in scene `scene`, if it has
/// one. The renderer's pick keys (`anim_timeline::split_key`) are such pairs.
#[must_use]
pub fn instance_name(doc: &Document, scene: usize, frame: u32, layer: usize, element: usize) -> Option<String> {
    let tl = doc.scenes.get(scene)?;
    let kf = tl.layers.get(layer)?.keyframe_at(frame)?;
    match kf.elements.get(element)? {
        Element::SymbolInstance(i) => i.name.clone(),
        _ => None,
    }
}

/// A listener registered by a script.
#[derive(Debug, Clone, PartialEq)]
struct Listener {
    target: Vec<String>,
    event: String,
    func: Func,
}

/// Where a movie clip's playhead is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipState {
    /// 0-based.
    pub frame: u32,
    pub playing: bool,
}

type Run<T> = Result<T, String>;

enum Flow {
    Next,
    Return(Value),
}

/// The runtime state of a preview.
#[derive(Debug, Clone)]
pub struct Player {
    version: Version,
    scene: usize,
    frame: u32,
    playing: bool,
    scripts: HashMap<(usize, usize, u32), Script>,
    globals: HashMap<String, Value>,
    locals: Vec<HashMap<String, Value>>,
    listeners: Vec<Listener>,
    clips: HashMap<Vec<String>, ClipState>,
    output: Vec<String>,
    queue_enter: bool,
    steps: u32,
    depth: u32,
}

impl Player {
    /// A player for `doc`, with every frame script parsed and the playhead on the first frame of the open scene.
    /// Call [`Player::start`] to run that frame's scripts.
    #[must_use]
    pub fn new(doc: &Document) -> Self {
        let mut scripts = HashMap::new();
        for (s, scene) in doc.scenes.iter().enumerate() {
            for (l, layer) in scene.layers.iter().enumerate() {
                for f in &layer.frames {
                    if let Some(src) = f.actionscript.as_deref().filter(|t| !t.trim().is_empty()) {
                        scripts.insert((s, l, f.index), parse(src));
                    }
                }
            }
        }
        let scene = doc.current_timeline.min(doc.scenes.len().saturating_sub(1));
        Self {
            version: script_version(doc),
            scene,
            frame: 0,
            playing: true,
            scripts,
            globals: HashMap::new(),
            locals: Vec::new(),
            listeners: Vec::new(),
            clips: HashMap::new(),
            output: Vec::new(),
            queue_enter: false,
            steps: 0,
            depth: 0,
        }
    }

    /// The `ActionScript` version the document uses.
    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// The scene and 0-based frame the playhead is on.
    #[must_use]
    pub const fn position(&self) -> (usize, u32) {
        (self.scene, self.frame)
    }

    /// False after `stop()` or `gotoAndStop`.
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playing
    }

    /// The playhead of a child clip, once a script has touched it.
    #[must_use]
    pub fn clip_state(&self, path: &[String]) -> Option<ClipState> {
        self.clips.get(path).copied()
    }

    /// Lines written by `trace` and runtime errors, oldest first.
    #[must_use]
    pub fn output(&self) -> &[String] {
        &self.output
    }

    /// Takes the output written so far.
    pub fn take_output(&mut self) -> Vec<String> {
        std::mem::take(&mut self.output)
    }

    /// Every parse diagnostic of every script, as `(scene, layer, frame, line, message)`.
    #[must_use]
    pub fn diagnostics(&self) -> Vec<(usize, usize, u32, u32, String)> {
        let mut out: Vec<_> = self.scripts.iter().flat_map(|(k, s)| s.diagnostics.iter().map(move |d| (k.0, k.1, k.2, d.line, d.message.clone()))).collect();
        out.sort();
        out
    }

    /// The names of the instances on the current frame that have a listener for `event`.
    #[must_use]
    pub fn listening(&self, doc: &Document, event: &str) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for l in self.listeners.iter().filter(|l| l.event == event) {
            if let [name] = l.target.as_slice()
                && self.find_instance(doc, name, self.frame).is_some()
                && !names.contains(name)
            {
                names.push(name.clone());
            }
        }
        names
    }

    /// Enters the current frame: runs its scripts.
    pub fn start(&mut self, doc: &Document) {
        self.queue_enter = true;
        self.run_queued(doc);
    }

    /// One frame tick: moves the playhead on when playing, fires `enterFrame` listeners and runs the scripts of the
    /// frame entered.
    pub fn advance(&mut self, doc: &Document) {
        if self.playing {
            self.step_forward(doc);
            self.queue_enter = true;
        }
        let targets: Vec<Listener> = self.listeners.iter().filter(|l| l.event == "enterFrame").cloned().collect();
        for l in targets {
            self.invoke(doc, &l.func, &l.target, "enterFrame");
        }
        self.run_queued(doc);
    }

    /// Delivers `event` (`click`, `mouseDown`, `rollOver`, ...) to the listeners of instance `name`.
    pub fn dispatch(&mut self, doc: &Document, name: &str, event: &str) {
        if self.find_instance(doc, name, self.frame).is_none() {
            return;
        }
        let target = vec![name.to_owned()];
        let hits: Vec<Listener> = self.listeners.iter().filter(|l| l.target == target && l.event == event).cloned().collect();
        for l in hits {
            self.invoke(doc, &l.func, &l.target, event);
        }
        self.run_queued(doc);
    }

    fn say(&mut self, line: String) {
        if self.output.len() >= OUTPUT_LIMIT {
            self.output.remove(0);
        }
        self.output.push(line);
    }

    fn step_forward(&mut self, doc: &Document) {
        let len = scene_frame_count(doc, self.scene);
        let next = self.frame.saturating_add(1);
        if next < len {
            self.frame = next;
        } else if self.scene.saturating_add(1) < doc.scenes.len() {
            self.scene = self.scene.saturating_add(1);
            self.frame = 0;
        } else {
            self.scene = 0;
            self.frame = 0;
        }
    }

    fn run_queued(&mut self, doc: &Document) {
        let mut chain = 0_u32;
        while self.queue_enter && chain < CHAIN_LIMIT {
            self.queue_enter = false;
            chain = chain.saturating_add(1);
            let layers = doc.scenes.get(self.scene).map_or(0, |t| t.layers.len());
            let here = (self.scene, self.frame);
            // Bottom layer first.
            for layer in (0..layers).rev() {
                let Some(script) = self.scripts.get(&(here.0, layer, here.1)).cloned() else { continue };
                self.run_script(doc, &script);
                if (self.scene, self.frame) != here {
                    break;
                }
            }
        }
        self.queue_enter = false;
    }

    fn run_script(&mut self, doc: &Document, script: &Script) {
        self.steps = 0;
        self.depth = 0;
        self.locals.clear();
        if let Err(e) = self.exec_block(doc, &script.stmts) {
            self.say(format!("Error: {e}"));
        }
        self.locals.clear();
    }

    fn invoke(&mut self, doc: &Document, func: &Func, target: &[String], event: &str) {
        self.steps = 0;
        self.depth = 0;
        self.locals.clear();
        let arg = Value::Event { kind: event.to_owned(), target: target.to_vec() };
        if let Err(e) = self.call_func(doc, func, &[arg]) {
            self.say(format!("Error: {e}"));
        }
        self.locals.clear();
    }

    // ---- timelines ----

    fn find_instance<'a>(&self, doc: &'a Document, name: &str, frame: u32) -> Option<&'a Symbol> {
        let tl = doc.scenes.get(self.scene)?;
        find_in(doc, tl, name, Some(frame))
    }

    /// The timeline a clip path names: the current scene for the empty path, else the symbol of the named instance.
    fn timeline_of<'a>(&self, doc: &'a Document, path: &[String]) -> Option<&'a Timeline> {
        let mut tl = doc.scenes.get(self.scene)?;
        for (depth, name) in path.iter().enumerate() {
            let frame = if depth == 0 { Some(self.frame) } else { None };
            let sym = find_in(doc, tl, name, frame)?;
            tl = &sym.timeline;
        }
        Some(tl)
    }

    fn clip_frame(&self, path: &[String]) -> u32 {
        if path.is_empty() { self.frame } else { self.clips.get(path).map_or(0, |c| c.frame) }
    }

    fn resolve_frame(tl: &Timeline, arg: &Value) -> Option<u32> {
        let len = tl.frame_count().max(1);
        match arg {
            Value::Num(n) if n.is_finite() => Some(round_f64_to_u32((*n - 1.0).max(0.0)).min(len.saturating_sub(1))),
            Value::Str(label) => {
                if let Some(f) = label_frame(tl, label) {
                    return Some(f);
                }
                // AS2 and AS3 both accept a frame number given as text.
                label.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(|n| round_f64_to_u32((n - 1.0).max(0.0)).min(len.saturating_sub(1)))
            }
            _ => None,
        }
    }

    /// `gotoAndPlay` / `gotoAndStop` on a clip.
    fn goto(&mut self, doc: &Document, path: &[String], args: &[Value], play: bool) -> Run<()> {
        let Some(first) = args.first() else { return Err("gotoAndPlay needs a frame".to_owned()) };
        if path.is_empty() {
            let scene = match args.get(1) {
                Some(Value::Str(name)) => doc.scenes.iter().position(|s| &s.name == name).ok_or_else(|| format!("scene \"{name}\" not found"))?,
                _ => self.scene,
            };
            let tl = doc.scenes.get(scene).ok_or_else(|| "no such scene".to_owned())?;
            let frame = Self::resolve_frame(tl, first).ok_or_else(|| format!("frame {} not found", first.to_text()))?;
            self.move_root(scene, frame, play);
            return Ok(());
        }
        let tl = self.timeline_of(doc, path).ok_or_else(|| format!("{} is not on the stage", path.join(".")))?;
        let frame = Self::resolve_frame(tl, first).ok_or_else(|| format!("frame {} not found", first.to_text()))?;
        self.clips.insert(path.to_vec(), ClipState { frame, playing: play });
        Ok(())
    }

    const fn move_root(&mut self, scene: usize, frame: u32, play: bool) {
        self.scene = scene;
        self.frame = frame;
        self.playing = play;
        self.queue_enter = true;
    }

    fn control(&mut self, doc: &Document, path: &[String], method: &str, args: &[Value]) -> Run<Value> {
        let is_root = path.is_empty();
        match method {
            "stop" => {
                if is_root {
                    self.playing = false;
                } else {
                    let frame = self.clip_frame(path);
                    self.clips.insert(path.to_vec(), ClipState { frame, playing: false });
                }
            }
            "play" => {
                if is_root {
                    self.playing = true;
                } else {
                    let frame = self.clip_frame(path);
                    self.clips.insert(path.to_vec(), ClipState { frame, playing: true });
                }
            }
            "gotoAndPlay" => self.goto(doc, path, args, true)?,
            "gotoAndStop" => self.goto(doc, path, args, false)?,
            "nextFrame" | "prevFrame" => {
                let len = self.timeline_of(doc, path).map_or(1, Timeline::frame_count).max(1);
                let here = self.clip_frame(path);
                let to = if method == "nextFrame" { here.saturating_add(1).min(len.saturating_sub(1)) } else { here.saturating_sub(1) };
                if is_root {
                    self.move_root(self.scene, to, false);
                } else {
                    self.clips.insert(path.to_vec(), ClipState { frame: to, playing: false });
                }
            }
            "nextScene" | "prevScene" if is_root => {
                let to = if method == "nextScene" { self.scene.checked_add(1).filter(|s| *s < doc.scenes.len()) } else { self.scene.checked_sub(1) };
                if let Some(scene) = to {
                    self.move_root(scene, 0, true);
                }
            }
            _ => return Err(format!("{method}() is not supported on {}", Value::Clip(path.to_vec()).to_text())),
        }
        Ok(Value::Undefined)
    }

    // ---- statements ----

    fn exec_block(&mut self, doc: &Document, stmts: &[Stmt]) -> Run<Flow> {
        for s in stmts {
            if let Stmt::Function(f) = s
                && let Some(name) = &f.name
            {
                self.declare(name, Value::Func(Box::new(f.clone())));
            }
        }
        for s in stmts {
            if let Flow::Return(v) = self.exec(doc, s)? {
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Next)
    }

    fn tick(&mut self) -> Run<()> {
        self.steps = self.steps.saturating_add(1);
        if self.steps > STEP_LIMIT { Err("the script ran too long and was stopped".to_owned()) } else { Ok(()) }
    }

    fn exec(&mut self, doc: &Document, s: &Stmt) -> Run<Flow> {
        self.tick()?;
        match s {
            Stmt::Expr(e) => {
                self.eval(doc, e)?;
            }
            Stmt::Var { name, init } => match init {
                Some(e) => {
                    let v = self.eval(doc, e)?;
                    self.declare(name, v);
                }
                None => {
                    if self.lookup(name).is_none() {
                        self.declare(name, Value::Undefined);
                    }
                }
            },
            Stmt::If { cond, then, otherwise } => {
                let branch = if self.eval(doc, cond)?.truthy() { then } else { otherwise };
                return self.exec_block(doc, branch);
            }
            Stmt::Block(b) => return self.exec_block(doc, b),
            Stmt::Function(_) | Stmt::Unsupported { .. } => {}
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(doc, e)?,
                    None => Value::Undefined,
                };
                return Ok(Flow::Return(v));
            }
        }
        Ok(Flow::Next)
    }

    fn declare(&mut self, name: &str, v: Value) {
        match self.locals.last_mut() {
            Some(frame) => frame.insert(name.to_owned(), v),
            None => self.globals.insert(name.to_owned(), v),
        };
    }

    fn lookup(&self, name: &str) -> Option<&Value> {
        self.locals.last().and_then(|f| f.get(name)).or_else(|| self.globals.get(name))
    }

    fn assign_name(&mut self, name: &str, v: Value) {
        if let Some(frame) = self.locals.last_mut()
            && frame.contains_key(name)
        {
            frame.insert(name.to_owned(), v);
        } else {
            self.globals.insert(name.to_owned(), v);
        }
    }

    // ---- expressions ----

    fn eval(&mut self, doc: &Document, e: &Expr) -> Run<Value> {
        self.tick()?;
        Ok(match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Null => Value::Undefined,
            Expr::This => Value::Clip(Vec::new()),
            Expr::Ident(name) => self.ident(doc, name)?,
            Expr::Member(base, name) => {
                let b = self.eval(doc, base)?;
                self.member(doc, &b, name)?
            }
            Expr::Call(callee, args) => self.call(doc, callee, args)?,
            Expr::Unary(op, x) => {
                let v = self.eval(doc, x)?;
                match op {
                    UnOp::Not => Value::Bool(!v.truthy()),
                    UnOp::Neg => Value::Num(-v.to_num()),
                }
            }
            Expr::Binary(op, a, b) => self.binary(doc, *op, a, b)?,
            Expr::Assign(target, op, value) => {
                let v = self.eval(doc, value)?;
                let v = match op {
                    Some(op) => {
                        let old = self.eval(doc, target)?;
                        arith(*op, &old, &v)
                    }
                    None => v,
                };
                self.assign(doc, target, v.clone())?;
                v
            }
            Expr::Update { target, delta, prefix } => {
                let old = self.eval(doc, target)?.to_num();
                let new = old + delta;
                self.assign(doc, target, Value::Num(new))?;
                Value::Num(if *prefix { new } else { old })
            }
            Expr::Function(f) => Value::Func(f.clone()),
        })
    }

    fn ident(&self, doc: &Document, name: &str) -> Run<Value> {
        if let Some(v) = self.lookup(name) {
            return Ok(v.clone());
        }
        match name {
            "root" | "_root" | "parent" | "_parent" => return Ok(Value::Clip(Vec::new())),
            "MouseEvent" | "Event" => return Ok(Value::Class(name.to_owned())),
            "currentFrame" | "_currentframe" | "totalFrames" | "_totalframes" | "currentLabel" => {
                return self.member(doc, &Value::Clip(Vec::new()), name);
            }
            _ => {}
        }
        if self.find_instance(doc, name, self.frame).is_some() {
            return Ok(Value::Clip(vec![name.to_owned()]));
        }
        Err(format!("ReferenceError: `{name}` is not defined"))
    }

    fn member(&self, doc: &Document, base: &Value, name: &str) -> Run<Value> {
        match base {
            Value::Class(class) => {
                let text = match (class.as_str(), name) {
                    ("MouseEvent", "CLICK") => "click",
                    ("MouseEvent", "MOUSE_DOWN") => "mouseDown",
                    ("MouseEvent", "MOUSE_UP") => "mouseUp",
                    ("MouseEvent", "MOUSE_OVER") => "mouseOver",
                    ("MouseEvent", "MOUSE_OUT") => "mouseOut",
                    ("MouseEvent", "ROLL_OVER") => "rollOver",
                    ("MouseEvent", "ROLL_OUT") => "rollOut",
                    ("MouseEvent", "DOUBLE_CLICK") => "doubleClick",
                    ("Event", "ENTER_FRAME") => "enterFrame",
                    _ => return Err(format!("{class}.{name} is not supported")),
                };
                Ok(Value::Str(text.to_owned()))
            }
            Value::Event { kind, target } => match name {
                "type" => Ok(Value::Str(kind.clone())),
                "target" | "currentTarget" => Ok(Value::Clip(target.clone())),
                _ => Err(format!("event.{name} is not supported")),
            },
            Value::Clip(path) => {
                let tl = self.timeline_of(doc, path);
                let count = |tl: Option<&Timeline>| f64::from(tl.map_or(1, Timeline::frame_count).max(1));
                match name {
                    "currentFrame" | "_currentframe" => Ok(Value::Num(f64::from(self.clip_frame(path)).max(0.0) + 1.0)),
                    "totalFrames" | "_totalframes" => Ok(Value::Num(count(tl))),
                    "currentLabel" => Ok(tl.and_then(|t| label_at(t, self.clip_frame(path))).map_or(Value::Undefined, Value::Str)),
                    "name" => Ok(path.last().map_or(Value::Undefined, |n| Value::Str(n.clone()))),
                    "root" | "_root" => Ok(Value::Clip(Vec::new())),
                    "parent" | "_parent" => Ok(Value::Clip(path.split_last().map(|(_, rest)| rest.to_vec()).unwrap_or_default())),
                    _ => {
                        let at = path.is_empty().then_some(self.frame);
                        let exists = tl.is_some_and(|t| find_in(doc, t, name, at).is_some());
                        if exists {
                            let mut child = path.clone();
                            child.push(name.to_owned());
                            Ok(Value::Clip(child))
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                }
            }
            other => Err(format!("cannot read `{name}` of {}", other.to_text())),
        }
    }

    fn assign(&mut self, doc: &Document, target: &Expr, v: Value) -> Run<()> {
        match target {
            Expr::Ident(name) => {
                self.assign_name(name, v);
                Ok(())
            }
            Expr::Member(base, name) => {
                let Value::Clip(path) = self.eval(doc, base)? else { return Err(format!("cannot set `{name}`")) };
                let event = match name.as_str() {
                    "onPress" => Some("mouseDown"),
                    "onRelease" => Some("click"),
                    "onRollOver" => Some("rollOver"),
                    "onRollOut" => Some("rollOut"),
                    "onEnterFrame" => Some("enterFrame"),
                    _ => None,
                };
                let Some(event) = event else {
                    self.say(format!("Warning: setting `{name}` is not supported by the preview"));
                    return Ok(());
                };
                self.listeners.retain(|l| !(l.target == path && l.event == event));
                if let Value::Func(f) = v {
                    self.listeners.push(Listener { target: path, event: event.to_owned(), func: *f });
                }
                Ok(())
            }
            _ => Err("only names and properties can be assigned to".to_owned()),
        }
    }

    fn binary(&mut self, doc: &Document, op: BinOp, a: &Expr, b: &Expr) -> Run<Value> {
        let left = self.eval(doc, a)?;
        match op {
            BinOp::And => {
                return if left.truthy() { self.eval(doc, b) } else { Ok(left) };
            }
            BinOp::Or => {
                return if left.truthy() { Ok(left) } else { self.eval(doc, b) };
            }
            _ => {}
        }
        let right = self.eval(doc, b)?;
        Ok(arith(op, &left, &right))
    }

    fn call(&mut self, doc: &Document, callee: &Expr, args: &[Expr]) -> Run<Value> {
        let mut values = Vec::with_capacity(args.len());
        for a in args {
            values.push(self.eval(doc, a)?);
        }
        match callee {
            Expr::Member(base, method) => {
                let b = self.eval(doc, base)?;
                self.call_method(doc, &b, method, &values)
            }
            Expr::Ident(name) => {
                if let Some(Value::Func(f)) = self.lookup(name).cloned() {
                    return self.call_func(doc, &f, &values);
                }
                self.call_global(doc, name, values)
            }
            other => match self.eval(doc, other)? {
                Value::Func(f) => self.call_func(doc, &f, &values),
                v => Err(format!("{} is not a function", v.to_text())),
            },
        }
    }

    fn call_global(&mut self, doc: &Document, name: &str, args: Vec<Value>) -> Run<Value> {
        match name {
            "stop" | "play" | "gotoAndPlay" | "gotoAndStop" | "nextFrame" | "prevFrame" | "nextScene" | "prevScene" => self.control(doc, &[], name, &args),
            "trace" => {
                let line = args.iter().map(Value::to_text).collect::<Vec<_>>().join(" ");
                self.say(line);
                Ok(Value::Undefined)
            }
            "MovieClip" | "SimpleButton" => Ok(args.into_iter().next().unwrap_or_default()),
            "Number" | "int" | "uint" => Ok(Value::Num(args.first().map_or(f64::NAN, Value::to_num))),
            "String" => Ok(Value::Str(args.first().map(Value::to_text).unwrap_or_default())),
            "Boolean" => Ok(Value::Bool(args.first().is_some_and(Value::truthy))),
            "addEventListener" | "removeEventListener" => self.call_method(doc, &Value::Clip(Vec::new()), name, &args),
            _ => Err(format!("{name}() is not supported")),
        }
    }

    fn call_method(&mut self, doc: &Document, base: &Value, method: &str, args: &[Value]) -> Run<Value> {
        match base {
            Value::Clip(path) => match method {
                "addEventListener" => {
                    let (Some(Value::Str(event)), Some(Value::Func(f))) = (args.first(), args.get(1)) else {
                        return Err("addEventListener needs an event type and a function".to_owned());
                    };
                    let listener = Listener { target: path.clone(), event: event.clone(), func: (**f).clone() };
                    if !self.listeners.contains(&listener) {
                        self.listeners.push(listener);
                    }
                    Ok(Value::Undefined)
                }
                "removeEventListener" => {
                    if let (Some(Value::Str(event)), Some(Value::Func(f))) = (args.first(), args.get(1)) {
                        self.listeners.retain(|l| !(l.target == *path && l.event == *event && l.func == **f));
                    }
                    Ok(Value::Undefined)
                }
                _ => self.control(doc, path, method, args),
            },
            Value::Event { .. } if matches!(method, "stopPropagation" | "preventDefault" | "stopImmediatePropagation") => Ok(Value::Undefined),
            other => Err(format!("{method}() is not supported on {}", other.to_text())),
        }
    }

    fn call_func(&mut self, doc: &Document, f: &Func, args: &[Value]) -> Run<Value> {
        if self.depth >= CALL_DEPTH_LIMIT {
            return Err("call stack too deep".to_owned());
        }
        self.depth = self.depth.saturating_add(1);
        let mut frame = HashMap::new();
        for (i, p) in f.params.iter().enumerate() {
            frame.insert(p.clone(), args.get(i).cloned().unwrap_or_default());
        }
        self.locals.push(frame);
        let flow = self.exec_block(doc, &f.body);
        self.locals.pop();
        self.depth = self.depth.saturating_sub(1);
        Ok(match flow? {
            Flow::Return(v) => v,
            Flow::Next => Value::Undefined,
        })
    }
}

fn arith(op: BinOp, a: &Value, b: &Value) -> Value {
    match op {
        BinOp::Add => {
            if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
                Value::Str(format!("{}{}", a.to_text(), b.to_text()))
            } else {
                Value::Num(a.to_num() + b.to_num())
            }
        }
        BinOp::Sub => Value::Num(a.to_num() - b.to_num()),
        BinOp::Mul => Value::Num(a.to_num() * b.to_num()),
        BinOp::Div => Value::Num(a.to_num() / b.to_num()),
        BinOp::Mod => Value::Num(a.to_num() % b.to_num()),
        BinOp::Eq => Value::Bool(loose_equals(a, b)),
        BinOp::Ne => Value::Bool(!loose_equals(a, b)),
        BinOp::StrictEq => Value::Bool(strict_equals(a, b)),
        BinOp::StrictNe => Value::Bool(!strict_equals(a, b)),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let ord = match (a, b) {
                (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
                _ => a.to_num().partial_cmp(&b.to_num()),
            };
            Value::Bool(ord.is_some_and(|o| match op {
                BinOp::Lt => o.is_lt(),
                BinOp::Le => o.is_le(),
                BinOp::Gt => o.is_gt(),
                _ => o.is_ge(),
            }))
        }
        BinOp::And => Value::Bool(a.truthy() && b.truthy()),
        BinOp::Or => Value::Bool(a.truthy() || b.truthy()),
    }
}

/// The first frame carrying `label`.
fn label_frame(tl: &Timeline, label: &str) -> Option<u32> {
    tl.layers.iter().flat_map(|l| l.frames.iter()).filter(|f| f.label.as_deref() == Some(label)).map(|f| f.index).min()
}

/// The label in force at `frame`: the one on the nearest keyframe at or before it.
fn label_at(tl: &Timeline, frame: u32) -> Option<String> {
    tl.layers
        .iter()
        .flat_map(|l| l.frames.iter())
        .filter(|f| f.index <= frame)
        .filter_map(|f| f.label.as_ref().map(|l| (f.index, l)))
        .max_by_key(|(i, _)| *i)
        .map(|(_, l)| l.clone())
}

/// The symbol of the instance called `name` on `tl`, at `frame` when given, else on any frame.
fn find_in<'a>(doc: &'a Document, tl: &Timeline, name: &str, frame: Option<u32>) -> Option<&'a Symbol> {
    for layer in &tl.layers {
        let frames: Vec<&anim_scene::timeline::Frame> = frame.map_or_else(|| layer.frames.iter().collect(), |f| layer.keyframe_at(f).into_iter().collect());
        for kf in frames {
            for el in &kf.elements {
                if let Element::SymbolInstance(i) = el
                    && i.name.as_deref() == Some(name)
                {
                    return doc.library.find_symbol(&i.library_item_name);
                }
            }
        }
    }
    None
}

/// Number of scenes' frames, for callers that size a timeline slider.
#[must_use]
pub fn scene_frame_count(doc: &Document, scene: usize) -> u32 {
    doc.scenes.get(scene).map_or(1, Timeline::frame_count).max(1)
}
