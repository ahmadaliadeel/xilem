// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! An index over an AccessKit tree, with absolute geometry.

use std::collections::HashMap;

use masonry::accesskit::{Node, NodeId, Role, TextDirection, TreeUpdate};
use masonry::kurbo::{Affine, Rect};

/// One line of text of a text widget, in capture (window) coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    /// Top of the line box.
    pub y0: f64,
    /// Bottom of the line box.
    pub y1: f64,
    /// Text of the line.
    pub text: String,
    /// Whether the line is right-to-left.
    pub rtl: bool,
}

/// An index over a complete AccessKit tree, as returned by
/// [`RenderRoot::full_access_tree`](masonry::app::RenderRoot::full_access_tree).
///
/// Node ids of widgets are their raw [`WidgetId`](masonry::core::WidgetId)s.
#[derive(Clone, Debug, Default)]
pub struct AccessIndex {
    nodes: HashMap<u64, Node>,
    parents: HashMap<u64, u64>,
    transforms: HashMap<u64, Affine>,
    root: Option<u64>,
}

fn to_affine(a: &masonry::accesskit::Affine) -> Affine {
    Affine::new(a.as_coeffs())
}

fn to_rect(r: masonry::accesskit::Rect) -> Rect {
    Rect::new(r.x0, r.y0, r.x1, r.y1)
}

impl AccessIndex {
    /// Builds an index from a complete tree update.
    pub fn new(update: &TreeUpdate) -> Self {
        let mut index = Self {
            root: update.tree.as_ref().map(|tree| tree.root.0),
            ..Self::default()
        };
        for (id, node) in &update.nodes {
            for child in node.children() {
                index.parents.insert(child.0, id.0);
            }
            index.nodes.insert(id.0, node.clone());
        }
        if index.root.is_none() {
            index.root = index
                .nodes
                .keys()
                .copied()
                .find(|id| !index.parents.contains_key(id));
        }
        if let Some(root) = index.root {
            index.compute_transforms(root, Affine::IDENTITY);
        }
        index
    }

    fn compute_transforms(&mut self, root: u64, base: Affine) {
        let mut stack = vec![(root, base)];
        while let Some((id, parent)) = stack.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            let transform = parent * node.transform().map(to_affine).unwrap_or_default();
            self.transforms.insert(id, transform);
            for child in node.children() {
                stack.push((child.0, transform));
            }
        }
    }

    /// The root node id.
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    /// Returns the node with the given id.
    pub fn node(&self, id: u64) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Returns the parent of a node.
    pub fn parent(&self, id: u64) -> Option<u64> {
        self.parents.get(&id).copied()
    }

    /// Returns the children of a node, in order.
    pub fn children(&self, id: u64) -> impl Iterator<Item = u64> + '_ {
        self.nodes
            .get(&id)
            .into_iter()
            .flat_map(|node| node.children().iter().map(|c: &NodeId| c.0))
    }

    /// Returns the role of a node.
    pub fn role(&self, id: u64) -> Option<Role> {
        self.nodes.get(&id).map(Node::role)
    }

    /// Transform from a node's coordinate space to capture coordinates.
    pub fn transform(&self, id: u64) -> Affine {
        self.transforms.get(&id).copied().unwrap_or_default()
    }

    /// Axis-aligned bounds of a node in capture coordinates.
    pub fn bounds(&self, id: u64) -> Option<Rect> {
        let node = self.nodes.get(&id)?;
        let bounds = to_rect(node.bounds()?);
        Some(self.transform(id).transform_rect_bbox(bounds))
    }

    /// Whether the node or one of its ancestors is hidden.
    pub fn is_hidden(&self, mut id: u64) -> bool {
        loop {
            match self.nodes.get(&id) {
                Some(node) if node.is_hidden() => return true,
                _ => {}
            }
            match self.parent(id) {
                Some(parent) => id = parent,
                None => return false,
            }
        }
    }

    /// The language of a node, inherited from its ancestors.
    pub fn language(&self, mut id: u64) -> Option<&str> {
        loop {
            if let Some(lang) = self.nodes.get(&id).and_then(|n| n.language()) {
                return Some(lang);
            }
            id = self.parent(id)?;
        }
    }

    /// The lines of text of a text widget (from its `TextRun` children).
    ///
    /// Lines are merged from runs whose vertical extents overlap, and sorted top to bottom.
    pub fn text_lines(&self, id: u64) -> Vec<TextLine> {
        let mut lines: Vec<TextLine> = Vec::new();
        for child in self.children(id) {
            let Some(node) = self.nodes.get(&child) else {
                continue;
            };
            if node.role() != Role::TextRun {
                continue;
            }
            let Some(bounds) = self.bounds(child) else {
                continue;
            };
            let text = node.value().unwrap_or_default();
            let rtl = node.text_direction() == Some(TextDirection::RightToLeft);
            match lines
                .iter_mut()
                .find(|line| line.y0 < bounds.y1 && bounds.y0 < line.y1)
            {
                Some(line) => {
                    line.y0 = line.y0.min(bounds.y0);
                    line.y1 = line.y1.max(bounds.y1);
                    line.text.push_str(text);
                }
                None => lines.push(TextLine {
                    y0: bounds.y0,
                    y1: bounds.y1,
                    text: text.to_string(),
                    rtl,
                }),
            }
        }
        lines.sort_by(|a, b| a.y0.total_cmp(&b.y0));
        lines
    }

    /// All text of a node and its descendants (`TextRun` values), in tree order.
    pub fn text_content(&self, id: u64) -> String {
        let mut out = String::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if node.role() == Role::TextRun {
                out.push_str(node.value().unwrap_or_default());
            }
            let children: Vec<u64> = node.children().iter().map(|c| c.0).collect();
            stack.extend(children.into_iter().rev());
        }
        out
    }

    /// All nodes in depth-first tree order, starting at the root.
    pub fn depth_first(&self) -> Vec<u64> {
        let mut out = Vec::new();
        let Some(root) = self.root else {
            return out;
        };
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            out.push(id);
            let children: Vec<u64> = self.children(id).collect();
            stack.extend(children.into_iter().rev());
        }
        out
    }

    /// Nodes whose AccessKit class name is `class`.
    pub fn find_class(&self, class: &str) -> Vec<u64> {
        self.depth_first()
            .into_iter()
            .filter(|id| self.nodes[id].class_name() == Some(class))
            .collect()
    }

    /// The first node whose AccessKit author id is `author_id`.
    pub fn find_author_id(&self, author_id: &str) -> Option<u64> {
        self.depth_first()
            .into_iter()
            .find(|id| self.nodes[id].author_id() == Some(author_id))
    }
}
