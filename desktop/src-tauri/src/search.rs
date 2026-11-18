//! Whole-book search, run by the core's search engine so that the reader
//! finds what the terminal app finds. The text is handed to it one leaf
//! block at a time (a paragraph, a list item, a table cell), each filed
//! under the chapter block it sits in, which is where a result leads.

use bookokrat::markdown::{Block, Document, Inline, Node, TableCellContent, Text, TextOrInline};
use bookokrat::search_engine::{SearchEngine, SearchLine, SearchResultTarget};
use serde::Serialize;
use std::collections::HashMap;

/// Characters of a result shown either side of what was found.
const SNIPPET_LEAD: usize = 60;
const SNIPPET_LENGTH: usize = 220;

#[derive(Serialize)]
pub struct Hit {
    /// The chapter of a book, or the page of a paged document.
    pub section: usize,
    pub title: String,
    /// The chapter block the text is in; for a page, the line on it.
    pub node: usize,
    /// For a page: where the line lies from its top, as fractions of its height.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<(f32, f32)>,
    pub snippet: String,
    /// The characters of `snippet` that matched.
    pub marks: Vec<usize>,
}

pub fn plain_text(text: &Text) -> String {
    text.iter()
        .map(|item| match item {
            TextOrInline::Text(node) => node.content.clone(),
            TextOrInline::Inline(Inline::Link { text, .. }) => plain_text(text),
            TextOrInline::Inline(Inline::LineBreak | Inline::SoftBreak) => " ".to_string(),
            TextOrInline::Inline(_) => String::new(),
        })
        .collect()
}

fn collect(nodes: &[Node], top: usize, out: &mut Vec<SearchLine>) {
    for node in nodes {
        collect_block(&node.block, top, out);
    }
}

fn push(out: &mut Vec<SearchLine>, top: usize, text: String) {
    if !text.trim().is_empty() {
        out.push(SearchLine {
            text,
            node_index: top,
            y_bounds: None,
        });
    }
}

fn collect_block(block: &Block, top: usize, out: &mut Vec<SearchLine>) {
    match block {
        Block::Heading { content, .. } | Block::Paragraph { content } => {
            push(out, top, plain_text(content));
        }
        Block::CodeBlock { content, .. } => {
            for line in content.lines() {
                push(out, top, line.to_string());
            }
        }
        Block::Quote { content } | Block::EpubBlock { content, .. } => collect(content, top, out),
        Block::List { items, .. } => {
            for item in items {
                collect(&item.content, top, out);
            }
        }
        Block::Table { header, rows, .. } => {
            for row in header.iter().chain(rows) {
                for cell in &row.cells {
                    match &cell.content {
                        TableCellContent::Simple(text) => push(out, top, plain_text(text)),
                        TableCellContent::Rich(nodes) => collect(nodes, top, out),
                    }
                }
            }
        }
        Block::DefinitionList { items } => {
            for item in items {
                push(out, top, plain_text(&item.term));
                for definition in &item.definitions {
                    collect(definition, top, out);
                }
            }
        }
        Block::ThematicBreak => {}
    }
}

/// The text of a chapter file as lines to search, its blocks counted from `first`.
pub fn lines_of(document: &Document, first: usize) -> Vec<SearchLine> {
    let mut out = Vec::new();
    for (index, node) in document.blocks.iter().enumerate() {
        collect_block(&node.block, first + index, &mut out);
    }
    out
}

pub fn word_count(lines: &[SearchLine]) -> usize {
    lines
        .iter()
        .map(|line| line.text.split_whitespace().count())
        .sum()
}

/// A stretch of the line around the first match, with the matched
/// characters counted from the start of that stretch.
fn window(text: &str, positions: &[usize]) -> (String, Vec<usize>) {
    let chars: Vec<char> = text.chars().collect();
    let first = positions
        .iter()
        .copied()
        .min()
        .unwrap_or(0)
        .min(chars.len());
    let mut start = first.saturating_sub(SNIPPET_LEAD);
    // Start on a word, not inside one.
    while start > 0 && start < first && !chars[start - 1].is_whitespace() {
        start += 1;
    }
    let end = (start + SNIPPET_LENGTH).min(chars.len());
    let mut snippet: String = chars[start..end].iter().collect();
    let mut shift = 0;
    if start > 0 {
        snippet.insert(0, '…');
        shift = 1;
    }
    if end < chars.len() {
        snippet.push('…');
    }
    let mut marks: Vec<usize> = positions
        .iter()
        .filter(|p| **p >= start && **p < end)
        .map(|p| p - start + shift)
        .collect();
    marks.sort_unstable();
    marks.dedup();
    (snippet, marks)
}

/// The core's search engine over a book or document, with the text it
/// was given: the engine hands back only the start of a long line, while
/// what it found may lie anywhere in it.
pub struct Index {
    engine: SearchEngine,
    /// The lines of each chapter or page, in the order the engine has them.
    texts: HashMap<usize, Vec<String>>,
}

impl Index {
    pub fn of_chapters(chapters: Vec<(usize, String, Vec<SearchLine>)>) -> Self {
        let texts = chapters
            .iter()
            .map(|(chapter, _, lines)| (*chapter, texts_of(lines)))
            .collect();
        let mut engine = SearchEngine::new();
        engine.process_chapters(chapters);
        Self { engine, texts }
    }

    pub fn of_pages(pages: Vec<(usize, Vec<SearchLine>)>) -> Self {
        let texts = pages
            .iter()
            .map(|(page, lines)| (*page, texts_of(lines)))
            .collect();
        let mut engine = SearchEngine::new();
        engine.process_pdf_pages(pages);
        Self { engine, texts }
    }

    /// Runs a query. Results come back in reading order, best matches first
    /// within the limits the engine sets.
    pub fn run(&self, query: &str, page_heights: Option<&[f32]>) -> Vec<Hit> {
        let mut hits: Vec<Hit> = self
            .engine
            .search_fuzzy(query)
            .into_iter()
            .map(|result| {
                let (section, node, span) = match result.target {
                    SearchResultTarget::Epub {
                        chapter_index,
                        node_index,
                    } => (chapter_index, node_index, None),
                    SearchResultTarget::Pdf {
                        page_index,
                        line_index,
                        line_y_bounds: (y0, y1),
                    } => {
                        let height = page_heights
                            .and_then(|heights| heights.get(page_index))
                            .copied()
                            .filter(|h| *h > 0.0 && y1 > y0);
                        (page_index, line_index, height.map(|h| (y0 / h, y1 / h)))
                    }
                };
                let line = self
                    .texts
                    .get(&section)
                    .and_then(|lines| lines.get(result.line_number))
                    .unwrap_or(&result.snippet);
                let (snippet, marks) = window(line, &result.match_positions);
                Hit {
                    section,
                    title: result.section_title,
                    node,
                    span,
                    snippet,
                    marks,
                }
            })
            .collect();
        hits.sort_by_key(|hit| (hit.section, hit.node));
        hits
    }
}

fn texts_of(lines: &[SearchLine]) -> Vec<String> {
    lines.iter().map(|line| line.text.clone()).collect()
}
