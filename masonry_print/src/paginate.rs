// Copyright 2026 the Xilem Authors
// SPDX-License-Identifier: Apache-2.0

//! Splitting flowing content into pages without cutting lines of text.

use masonry::accesskit::Role;

use crate::access::AccessIndex;
use crate::widgets::{KEEP_TOGETHER_CLASS, PAGE_BREAK_CLASS};

/// A piece of content that must not be split across pages (e.g. a line of text).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atom {
    /// Top, in flow coordinates.
    pub y0: f64,
    /// Bottom, in flow coordinates.
    pub y1: f64,
    /// Whether this atom must be on the same page as the following one (e.g. a heading).
    pub keep_with_next: bool,
}

/// The vertical structure of flowing content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FlowGeometry {
    /// Total height of the content.
    pub height: f64,
    /// Unbreakable pieces of content.
    pub atoms: Vec<Atom>,
    /// Positions of forced page breaks.
    pub forced_breaks: Vec<f64>,
}

/// A vertical band of flowing content that is printed on one page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSlice {
    /// Top of the band, in flow coordinates.
    pub y0: f64,
    /// Bottom of the band, in flow coordinates.
    pub y1: f64,
}

const EPSILON: f64 = 1e-6;

fn is_container(role: Role) -> bool {
    matches!(
        role,
        Role::GenericContainer
            | Role::Group
            | Role::Section
            | Role::Paragraph
            | Role::List
            | Role::ListItem
            | Role::Table
            | Role::Row
            | Role::Cell
            | Role::ColumnHeader
            | Role::RowHeader
            | Role::Document
            | Role::Window
            | Role::Pane
            | Role::Link
            | Role::Figure
            | Role::Label
    )
}

/// Computes the unbreakable pieces of the content below `root` in the accessibility tree.
pub fn flow_geometry(access: &AccessIndex, root: u64, height: f64) -> FlowGeometry {
    let mut geometry = FlowGeometry {
        height,
        ..FlowGeometry::default()
    };
    let mut stack: Vec<u64> = access.children(root).collect();
    stack.reverse();
    while let Some(id) = stack.pop() {
        let Some(node) = access.node(id) else {
            continue;
        };
        let Some(bounds) = access.bounds(id) else {
            continue;
        };
        if node.class_name() == Some(PAGE_BREAK_CLASS) {
            geometry.forced_breaks.push(bounds.y0);
            continue;
        }
        if bounds.height() <= EPSILON {
            continue;
        }
        let role = node.role();
        if node.class_name() == Some(KEEP_TOGETHER_CLASS) || role == Role::Heading {
            geometry.atoms.push(Atom {
                y0: bounds.y0,
                y1: bounds.y1,
                keep_with_next: role == Role::Heading,
            });
            continue;
        }
        let lines = access.text_lines(id);
        if !lines.is_empty() {
            geometry.atoms.extend(lines.iter().map(|line| Atom {
                y0: line.y0,
                y1: line.y1,
                keep_with_next: false,
            }));
            continue;
        }
        let children: Vec<u64> = access.children(id).collect();
        if is_container(role) && !children.is_empty() {
            stack.extend(children.into_iter().rev());
        } else {
            geometry.atoms.push(Atom {
                y0: bounds.y0,
                y1: bounds.y1,
                keep_with_next: false,
            });
        }
    }
    geometry.forced_breaks.sort_by(f64::total_cmp);
    geometry
}

/// Merges atoms into blocks between which the content may be broken.
fn blocks(atoms: &[Atom]) -> Vec<(f64, f64)> {
    let mut atoms = atoms.to_vec();
    atoms.sort_by(|a, b| a.y0.total_cmp(&b.y0));
    let mut blocks: Vec<(f64, f64, bool)> = Vec::new();
    for atom in atoms {
        match blocks.last_mut() {
            // Overlapping atoms, or an atom that must stay with its predecessor.
            Some(last) if atom.y0 < last.1 - EPSILON || last.2 => {
                last.1 = last.1.max(atom.y1);
                last.2 = atom.keep_with_next;
            }
            _ => blocks.push((atom.y0, atom.y1, atom.keep_with_next)),
        }
    }
    blocks.into_iter().map(|(y0, y1, _)| (y0, y1)).collect()
}

/// Splits content into pages of height `page_height`.
///
/// Pages are only broken between blocks of content, at forced breaks, or (if a block is
/// taller than a page) at the page height.
pub fn paginate(geometry: &FlowGeometry, page_height: f64) -> Vec<PageSlice> {
    let blocks = blocks(&geometry.atoms);
    let end = geometry
        .height
        .max(blocks.iter().map(|b| b.1).fold(0.0, f64::max));
    let mut slices = Vec::new();
    let mut start = 0.0;
    let page_height = page_height.max(1.0);
    // Skips blank space at the top of a page.
    let skip_gap = |start: f64| {
        blocks
            .iter()
            .find(|b| b.1 > start + EPSILON)
            .map_or(end, |b| b.0.max(start))
    };
    while start < end - EPSILON {
        let limit = start + page_height;
        if let Some(forced) = geometry
            .forced_breaks
            .iter()
            .copied()
            .find(|b| *b > start + EPSILON && *b <= limit)
        {
            slices.push(PageSlice {
                y0: start,
                y1: forced,
            });
            start = skip_gap(forced);
            continue;
        }
        if limit >= end - EPSILON {
            slices.push(PageSlice { y0: start, y1: end });
            break;
        }
        let fitting = blocks
            .iter()
            .filter(|b| b.1 <= limit + EPSILON && b.1 > start + EPSILON)
            .map(|b| b.1)
            .fold(None, |acc: Option<f64>, y| {
                Some(acc.map_or(y, |a| a.max(y)))
            });
        let cut = match fitting {
            Some(block_end) => {
                // Keep the space after the block on this page, up to the next block.
                blocks
                    .iter()
                    .find(|b| b.0 >= block_end - EPSILON)
                    .map_or(block_end, |next| next.0.min(limit))
            }
            // A block taller than a page: it has to be cut.
            None => limit,
        };
        slices.push(PageSlice { y0: start, y1: cut });
        start = skip_gap(cut);
    }
    if slices.is_empty() {
        slices.push(PageSlice { y0: 0.0, y1: end });
    }
    slices
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(ys: &[(f64, f64)]) -> Vec<Atom> {
        ys.iter()
            .map(|&(y0, y1)| Atom {
                y0,
                y1,
                keep_with_next: false,
            })
            .collect()
    }

    fn geometry(atoms: Vec<Atom>, height: f64) -> FlowGeometry {
        FlowGeometry {
            height,
            atoms,
            forced_breaks: Vec::new(),
        }
    }

    #[test]
    fn breaks_between_lines() {
        // Lines of 20px, pages of 50px: two lines per page.
        let atoms = lines(&[(0., 20.), (20., 40.), (40., 60.), (60., 80.), (80., 100.)]);
        let slices = paginate(&geometry(atoms, 100.), 50.);
        assert_eq!(
            slices,
            vec![
                PageSlice { y0: 0., y1: 40. },
                PageSlice { y0: 40., y1: 80. },
                PageSlice { y0: 80., y1: 100. },
            ]
        );
    }

    #[test]
    fn never_cuts_a_line() {
        let atoms = lines(&[(0., 30.), (35., 65.), (70., 100.)]);
        for page_height in [31.0, 50.0, 66.0, 99.0] {
            for slice in paginate(&geometry(atoms.clone(), 100.), page_height) {
                for atom in &atoms {
                    let cut = atom.y0 < slice.y1 - EPSILON && atom.y1 > slice.y1 + EPSILON;
                    assert!(
                        !cut,
                        "{atom:?} cut by {slice:?} at page height {page_height}"
                    );
                }
            }
        }
    }

    #[test]
    fn forced_breaks_and_leading_gaps() {
        let mut g = geometry(lines(&[(0., 10.), (30., 40.)]), 40.);
        g.forced_breaks.push(20.);
        let slices = paginate(&g, 100.);
        assert_eq!(
            slices,
            vec![
                PageSlice { y0: 0., y1: 20. },
                PageSlice { y0: 30., y1: 40. }
            ]
        );
    }

    #[test]
    fn oversized_blocks_are_sliced() {
        let slices = paginate(&geometry(lines(&[(0., 250.)]), 250.), 100.);
        assert_eq!(slices.len(), 3);
        assert_eq!(slices[0], PageSlice { y0: 0., y1: 100. });
        assert_eq!(slices[2].y1, 250.);
    }

    #[test]
    fn headings_stay_with_the_next_block() {
        let mut atoms = lines(&[(0., 40.), (40., 50.), (50., 60.)]);
        atoms[1].keep_with_next = true;
        // The heading (40..50) would fit on the first page, but its paragraph doesn't.
        let slices = paginate(&geometry(atoms, 60.), 55.);
        assert_eq!(slices[0], PageSlice { y0: 0., y1: 40. });
        assert_eq!(slices[1], PageSlice { y0: 40., y1: 60. });
    }
}
