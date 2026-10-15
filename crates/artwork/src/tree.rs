//! The neutral layer tree every importer produces.

use std::fmt::Write as _;

use anim_core::{BlendMode, Point, Rect, Size};
use anim_display::{Image, Node};

/// A layered piece of artwork: a PSD, an Illustrator file, an SVG.
///
/// Children are bottom-most first (painter's order), the same convention as [`anim_display::DisplayList`], so
/// a node's last child is drawn on top. Names are kept exactly as the file has them: Character Animator puppet
/// tags (`+Head`, `Left Eye`, ...) live in them.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtworkTree {
    /// Canvas size in pixels (PSD document size, AI artboard in points, SVG viewport).
    pub size: Size,
    /// The unnamed top of the hierarchy. Its children are the file's top-level layers.
    pub root: ArtNode,
    /// Things the importer could not carry over (unsupported objects, features approximated).
    pub warnings: Vec<String>,
}

/// One layer, group or object.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtNode {
    /// Exact name from the file; empty when the file gives none.
    pub name: String,
    pub visible: bool,
    /// 0.0 ..= 1.0.
    pub opacity: f32,
    pub blend: BlendMode,
    /// PSD "clip to layer below": this node is drawn only where the closest preceding non-clipping sibling is.
    pub clipping: bool,
    /// Layer mask or clipping path, in canvas coordinates; content is shown only where it has coverage.
    pub mask: Option<ArtMask>,
    pub kind: ArtKind,
}

/// What a node holds.
#[derive(Debug, Clone, PartialEq)]
pub enum ArtKind {
    /// Children, bottom-most first.
    Group(Vec<ArtNode>),
    /// Premultiplied pixels with their top-left corner at `offset` on the canvas.
    Raster { image: Image, offset: Point },
    /// Resolved vector drawing in canvas coordinates (fills, strokes, gradients), with its bounding box.
    Vector { nodes: Vec<Node>, bounds: Rect },
}

/// A mask in canvas coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum ArtMask {
    /// The alpha channel of `image` is the coverage.
    Raster { image: Image, offset: Point },
    /// Anything painted by `nodes` has coverage (clipping paths).
    Vector { nodes: Vec<Node> },
}

impl ArtNode {
    fn with_kind(name: impl Into<String>, kind: ArtKind) -> Self {
        Self { name: name.into(), visible: true, opacity: 1.0, blend: BlendMode::Normal, clipping: false, mask: None, kind }
    }

    /// A visible, opaque, normal-blend group.
    #[must_use]
    pub fn group(name: impl Into<String>, children: Vec<Self>) -> Self {
        Self::with_kind(name, ArtKind::Group(children))
    }

    /// A visible, opaque, normal-blend raster layer.
    #[must_use]
    pub fn raster(name: impl Into<String>, image: Image, offset: Point) -> Self {
        Self::with_kind(name, ArtKind::Raster { image, offset })
    }

    /// A visible, opaque, normal-blend vector object.
    #[must_use]
    pub fn vector(name: impl Into<String>, nodes: Vec<Node>, bounds: Rect) -> Self {
        Self::with_kind(name, ArtKind::Vector { nodes, bounds })
    }

    /// Children (empty for rasters and vectors).
    #[must_use]
    pub fn children(&self) -> &[Self] {
        match &self.kind {
            ArtKind::Group(children) => children,
            ArtKind::Raster { .. } | ArtKind::Vector { .. } => &[],
        }
    }

    #[must_use]
    pub const fn is_group(&self) -> bool {
        matches!(self.kind, ArtKind::Group(_))
    }

    /// Short kind label: `group`, `raster` or `vector`.
    #[must_use]
    pub const fn kind_name(&self) -> &'static str {
        match self.kind {
            ArtKind::Group(_) => "group",
            ArtKind::Raster { .. } => "raster",
            ArtKind::Vector { .. } => "vector",
        }
    }

    /// Bounding box on the canvas of what this node holds itself (groups: the union of their children).
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        let mut out: Option<Rect> = None;
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            let own = match &node.kind {
                ArtKind::Group(children) => {
                    stack.extend(children.iter());
                    None
                }
                ArtKind::Raster { image, offset } => Some(Rect::new(offset.x, offset.y, offset.x + f64::from(image.width), offset.y + f64::from(image.height))),
                ArtKind::Vector { bounds, nodes } => (!nodes.is_empty()).then_some(*bounds),
            };
            if let Some(r) = own.filter(|r| r.width() > 0.0 && r.height() > 0.0) {
                out = Some(out.map_or(r, |o| o.union(r)));
            }
        }
        out
    }

    /// Calls `f(node, depth)` for this node (depth 0) and every descendant, parents before children, siblings in
    /// order. Iterative, so a hand-built deep tree cannot overflow the stack.
    pub fn walk(&self, mut f: impl FnMut(&Self, usize)) {
        let mut stack = vec![(self, 0_usize)];
        while let Some((node, depth)) = stack.pop() {
            f(node, depth);
            for child in node.children().iter().rev() {
                stack.push((child, depth.saturating_add(1)));
            }
        }
    }

    /// Number of nodes in this subtree, itself included.
    #[must_use]
    pub fn count(&self) -> usize {
        let mut n = 0_usize;
        self.walk(|_, _| n = n.saturating_add(1));
        n
    }

    /// First node (depth first, this node included) with exactly this name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Self> {
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            if node.name == name {
                return Some(node);
            }
            stack.extend(node.children().iter().rev());
        }
        None
    }

    /// Follows `names` down from this node: each name picks the first direct child with that name.
    #[must_use]
    pub fn find_path(&self, names: &[&str]) -> Option<&Self> {
        let mut node = self;
        for name in names {
            node = node.children().iter().find(|c| c.name == *name)?;
        }
        Some(node)
    }

    /// Names of every node in this subtree, depth first (this node's name first).
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        let mut out = Vec::new();
        let mut stack = vec![self];
        while let Some(node) = stack.pop() {
            out.push(node.name.as_str());
            stack.extend(node.children().iter().rev());
        }
        out
    }
}

impl ArtworkTree {
    /// Names of every node under the root, depth first.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.root.names()
    }

    /// First node anywhere in the tree with exactly this name.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&ArtNode> {
        self.root.find(name)
    }

    /// An indented text outline: one line per node with kind, size, visibility, opacity and blend. For logs, tests
    /// and tools that show the hierarchy.
    #[must_use]
    pub fn outline(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "artwork {}x{}", self.size.width, self.size.height);
        self.root.walk(|node, depth| {
            if depth == 0 {
                return;
            }
            for _ in 0..depth {
                out.push_str("  ");
            }
            let label = if node.name.is_empty() { "(unnamed)" } else { node.name.as_str() };
            let _ = write!(out, "{label} [{}", node.kind_name());
            match &node.kind {
                ArtKind::Group(children) => {
                    let _ = write!(out, " {}", children.len());
                }
                ArtKind::Raster { image, offset } => {
                    let _ = write!(out, " {}x{} @ {},{}", image.width, image.height, offset.x, offset.y);
                }
                ArtKind::Vector { nodes, .. } => {
                    let _ = write!(out, " {} nodes", nodes.len());
                }
            }
            if !node.visible {
                out.push_str(", hidden");
            }
            if node.opacity < 1.0 {
                let _ = write!(out, ", opacity {:.2}", node.opacity);
            }
            if node.blend != BlendMode::Normal {
                let _ = write!(out, ", {:?}", node.blend);
            }
            if node.clipping {
                out.push_str(", clipped");
            }
            if node.mask.is_some() {
                out.push_str(", masked");
            }
            out.push_str("]\n");
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ArtworkTree {
        let leaf = |n: &str| ArtNode::vector(n, Vec::new(), Rect::ZERO);
        let head = ArtNode::group("Head", vec![leaf("Left Eye"), leaf("Right Eye"), leaf("Mouth")]);
        ArtworkTree {
            size: Size::new(100.0, 50.0),
            root: ArtNode::group("root", vec![ArtNode::group("Body", vec![leaf("Torso")]), head]),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn find_and_path() {
        let t = sample();
        assert!(t.find("Mouth").is_some());
        assert!(t.find("Nose").is_none());
        assert_eq!(t.root.find_path(&["Head", "Right Eye"]).map(|n| n.name.as_str()), Some("Right Eye"));
        assert!(t.root.find_path(&["Head", "Torso"]).is_none());
    }

    #[test]
    fn names_are_depth_first_bottom_up() {
        let t = sample();
        assert_eq!(t.names(), ["root", "Body", "Torso", "Head", "Left Eye", "Right Eye", "Mouth"]);
        assert_eq!(t.root.count(), 7);
    }

    #[test]
    fn outline_lists_every_node() {
        let t = sample();
        let o = t.outline();
        assert!(o.contains("  Body [group 1]"));
        assert!(o.contains("    Left Eye [vector 0 nodes]"));
        assert_eq!(o.lines().count(), 7);
    }
}
