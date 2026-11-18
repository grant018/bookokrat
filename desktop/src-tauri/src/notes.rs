//! Highlights and notes, kept in the terminal app's own store through the
//! core's types, so that either app shows what the other wrote.
//!
//! A highlight or note covers one or more stretches of text, each inside a
//! single block and counted in characters from the block's start. The reader
//! draws the ones that lie in paragraphs and headings; what the terminal app
//! put on list items, quotations or code is listed but not drawn.

use bookokrat::annotations::HighlightColor;
use bookokrat::comments::{
    BlockAddress, BlockSubtarget, BookComments, Comment, CommentTarget, PdfSelectionRect, TextSlice,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One stretch of an annotation: a block of a chapter file, found by its
/// place among the file's blocks and then down through any wrapping blocks,
/// and a range of characters in it. No end means to the end of the block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slice {
    pub node: usize,
    #[serde(default)]
    pub path: Vec<usize>,
    pub start: usize,
    pub end: Option<usize>,
}

pub struct Annotation {
    pub id: String,
    /// The chapter file the annotation is in, as the book names it.
    pub href: String,
    pub color: Option<HighlightColor>,
    pub text: String,
    pub quote: Option<String>,
    pub updated: String,
    pub slices: Vec<Slice>,
    /// False when any part of it lies outside paragraphs and headings.
    pub drawable: bool,
}

pub fn load(book: &str, comments_dir: &Path) -> Result<BookComments, String> {
    BookComments::new(Path::new(book), Some(comments_dir))
        .map_err(|e| format!("Failed to load the notes for {book}: {e:#}"))
}

pub fn list(store: &BookComments) -> Vec<Annotation> {
    store
        .get_all_comments()
        .iter()
        .filter(|comment| comment.is_text())
        .map(|comment| {
            let mut drawable = true;
            let slices = comment
                .target
                .slices()
                .iter()
                .filter_map(|slice| match &slice.subtarget {
                    BlockSubtarget::Paragraph { word_range } => Some(Slice {
                        node: slice.block.node_index,
                        path: slice.block.child_path.clone(),
                        start: word_range.map_or(0, |(start, _)| start),
                        end: word_range.map(|(_, end)| end),
                    }),
                    _ => {
                        drawable = false;
                        None
                    }
                })
                .collect();
            Annotation {
                id: comment.id.clone(),
                href: comment.chapter_href.clone(),
                color: comment.highlight_color(),
                text: comment.content.clone(),
                quote: comment.quoted_text.clone(),
                updated: comment.updated_at.to_rfc3339(),
                slices,
                drawable,
            }
        })
        .collect()
}

/// The share of a new highlight or note that lies in one chapter file.
pub struct Piece {
    pub href: String,
    pub slices: Vec<Slice>,
    pub quote: Option<String>,
}

/// Adds a highlight, when a colour is given, or else a note, for each piece
/// of a selection. Nothing is added unless every piece can be.
pub fn add(
    store: &mut BookComments,
    pieces: Vec<Piece>,
    color: Option<HighlightColor>,
    text: String,
) -> Result<(), String> {
    let mut targets = Vec::with_capacity(pieces.len());
    for piece in pieces {
        let slices: Vec<TextSlice> = piece
            .slices
            .iter()
            .filter(|slice| slice.end.is_none_or(|end| end > slice.start))
            .map(|slice| {
                TextSlice::new_at(
                    BlockAddress {
                        node_index: slice.node,
                        child_path: slice.path.clone(),
                    },
                    BlockSubtarget::Paragraph {
                        word_range: Some((slice.start, slice.end.unwrap_or(usize::MAX))),
                    },
                )
            })
            .collect();
        if slices.is_empty() {
            continue;
        }
        let target = CommentTarget::from_slices(slices);
        // The terminal app draws both kinds in one layer and refuses overlaps;
        // an overlap written here would show there as one or the other.
        if store.has_overlapping_annotation(&piece.href, &target) {
            return Err("That passage already has a highlight or a note".to_string());
        }
        targets.push((piece.href, target, piece.quote));
    }
    if targets.is_empty() {
        return Err("There is no text to mark there".to_string());
    }
    for (href, target, quote) in targets {
        let comment = match color {
            Some(color) => Comment::new_highlight(href, target, color, Utc::now(), quote),
            None => Comment::with_quoted_text(href, target, text.clone(), Utc::now(), quote),
        };
        store
            .add_comment(comment)
            .map_err(|e| format!("Failed to save the notes: {e:#}"))?;
    }
    Ok(())
}

pub fn set_text(store: &mut BookComments, id: &str, text: String) -> Result<(), String> {
    store
        .update_comment_by_id(id, text)
        .map_err(|e| format!("Failed to save the note: {e:#}"))
}

pub fn set_color(store: &mut BookComments, id: &str, color: HighlightColor) -> Result<(), String> {
    store
        .set_highlight_color_by_id(id, color)
        .map_err(|e| format!("Failed to save the highlight: {e:#}"))
}

pub fn delete(store: &mut BookComments, id: &str) -> Result<(), String> {
    store
        .delete_comment_by_id(id)
        .map_err(|e| format!("Failed to save the notes: {e:#}"))
}

/// A highlight or note on a page of a PDF: the boxes it covers, in the
/// document's own units from the page's top left corner.
pub struct PageAnnotation {
    pub id: String,
    pub page: usize,
    pub color: Option<HighlightColor>,
    pub text: String,
    pub quote: Option<String>,
    pub updated: String,
    pub rects: Vec<PdfSelectionRect>,
}

pub fn list_paged(store: &BookComments) -> Vec<PageAnnotation> {
    store
        .get_all_comments()
        .iter()
        .filter_map(|comment| {
            let CommentTarget::Pdf { page, rects } = &comment.target else {
                return None;
            };
            Some(PageAnnotation {
                id: comment.id.clone(),
                page: *page,
                color: comment.highlight_color(),
                text: comment.content.clone(),
                quote: comment.quoted_text.clone(),
                updated: comment.updated_at.to_rfc3339(),
                rects: rects.clone(),
            })
        })
        .collect()
}

/// The name the document's notes are filed under: the one already in
/// use, so that notes written here join the terminal app's.
pub fn paged_name(store: &BookComments) -> Option<String> {
    store
        .get_all_comments()
        .iter()
        .find(|comment| comment.is_pdf())
        .map(|comment| comment.chapter_href.clone())
}

pub fn add_paged(
    store: &mut BookComments,
    name: &str,
    page: usize,
    rects: Vec<PdfSelectionRect>,
    color: Option<HighlightColor>,
    text: String,
    quote: Option<String>,
) -> Result<(), String> {
    if rects.is_empty() {
        return Err("There is no text to mark there".to_string());
    }
    let target = CommentTarget::pdf(page, rects);
    if store.has_overlapping_annotation(name, &target) {
        return Err("That passage already has a highlight or a note".to_string());
    }
    let comment = match color {
        Some(color) => Comment::new_highlight(name.to_string(), target, color, Utc::now(), quote),
        None => Comment::with_quoted_text(name.to_string(), target, text, Utc::now(), quote),
    };
    store
        .add_comment(comment)
        .map_err(|e| format!("Failed to save the notes: {e:#}"))
}
