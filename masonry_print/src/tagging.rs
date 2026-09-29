// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Building the PDF structure tree from the accessibility tree.

use std::num::NonZeroU16;

use imaging_krilla::krilla::tagging::{
    ListNumbering, Node, TableHeaderScope, Tag, TagGroup, TagKind,
};
use masonry::accesskit::Role;

use crate::access::AccessIndex;
use crate::sink::{CaptureTags, Class, classify};

/// The alternative text of an image-like node.
pub(crate) fn alt_text(access: &AccessIndex, id: u64) -> Option<String> {
    let node = access.node(id)?;
    node.description()
        .or(node.label())
        .filter(|alt| !alt.trim().is_empty())
        .map(str::to_string)
}

fn with_lang(tag: impl Into<TagKind>, lang: Option<&str>) -> TagKind {
    let mut kind: TagKind = tag.into();
    if let Some(lang) = lang {
        kind.as_any_mut().set_lang(Some(lang.to_string()));
    }
    kind
}

fn owner_tag(access: &AccessIndex, id: u64) -> TagKind {
    let node = access.node(id).expect("node exists");
    let lang = node.language();
    match node.role() {
        Role::Heading => {
            let level = node
                .level()
                .and_then(|l| u16::try_from(l).ok())
                .and_then(NonZeroU16::new)
                .unwrap_or(NonZeroU16::MIN);
            with_lang(Tag::Hn(level, None), lang)
        }
        Role::Link => with_lang(Tag::Link, lang),
        Role::Image | Role::Figure => with_lang(Tag::Figure(alt_text(access, id)), lang),
        _ => with_lang(Tag::P, lang),
    }
}

fn container_tag(access: &AccessIndex, id: u64) -> Option<TagKind> {
    let node = access.node(id)?;
    let lang = node.language();
    Some(match node.role() {
        Role::Table => with_lang(Tag::Table, lang),
        Role::Row => with_lang(Tag::TR, lang),
        Role::Cell => with_lang(Tag::TD, lang),
        Role::ColumnHeader => with_lang(Tag::TH(TableHeaderScope::Column), lang),
        Role::RowHeader => with_lang(Tag::TH(TableHeaderScope::Row), lang),
        Role::List => with_lang(Tag::L(ListNumbering::None), lang),
        Role::ListItem => with_lang(Tag::LI, lang),
        Role::Section => with_lang(Tag::Section, lang),
        _ => return None,
    })
}

fn leaves(tags: &mut CaptureTags, id: u64) -> Vec<Node> {
    tags.leaves
        .remove(&id)
        .unwrap_or_default()
        .into_iter()
        .map(Node::Leaf)
        .collect()
}

fn build(access: &AccessIndex, tags: &mut CaptureTags, id: u64) -> Vec<Node> {
    let children: Vec<u64> = access.children(id).collect();
    let mut child_nodes = Vec::new();
    for child in children {
        child_nodes.extend(build(access, tags, child));
    }
    match classify(access, id) {
        Class::Owner | Class::Text => {
            let mut content = leaves(tags, id);
            content.extend(child_nodes);
            if content.is_empty() {
                return Vec::new();
            }
            let tag = if classify(access, id) == Class::Owner {
                owner_tag(access, id)
            } else {
                with_lang(Tag::P, access.node(id).and_then(|n| n.language()))
            };
            vec![Node::Group(TagGroup::with_children(tag, content))]
        }
        Class::Container => {
            if child_nodes.is_empty() {
                return Vec::new();
            }
            let Some(tag) = container_tag(access, id) else {
                return child_nodes;
            };
            if access.role(id) == Some(Role::ListItem) {
                let body = TagGroup::with_children(Tag::LBody, child_nodes);
                return vec![Node::Group(TagGroup::with_children(
                    tag,
                    vec![Node::Group(body)],
                ))];
            }
            vec![Node::Group(TagGroup::with_children(tag, child_nodes))]
        }
        Class::Transparent => {
            // Text drawn by widgets without text semantics.
            let own = leaves(tags, id);
            if own.is_empty() {
                child_nodes
            } else {
                let mut nodes = vec![Node::Group(TagGroup::with_children(Tag::P, own))];
                nodes.extend(child_nodes);
                nodes
            }
        }
    }
}

/// Builds the structure elements for one capture, in reading (tree) order.
pub(crate) fn capture_structure(access: &AccessIndex, mut tags: CaptureTags) -> Vec<Node> {
    let Some(root) = access.root() else {
        return Vec::new();
    };
    let mut nodes = build(access, &mut tags, root);
    // Content attributed to nodes that aren't in the tree (shouldn't happen) is kept,
    // so every marked content sequence is referenced exactly once.
    let mut rest: Vec<_> = tags.leaves.into_iter().collect();
    rest.sort_by_key(|(id, _)| *id);
    for (_, ids) in rest {
        nodes.push(Node::Group(TagGroup::with_children(
            Tag::P,
            ids.into_iter().map(Node::Leaf).collect(),
        )));
    }
    nodes
}
