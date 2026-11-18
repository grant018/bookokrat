//! JSON shapes handed to the frontend. The core's Markdown AST stays the
//! source of truth; block order here matches `Document::blocks`, so a block's
//! position is the same node index the TUI stores comments and marks against.

use bookokrat::markdown::{
    Block, BlockAlign, Document, Inline, LinkType, ListKind, Node, Script, Style, TableAlignment,
    TableCell, TableCellContent, TableRow, Text, TextOrInline,
};
use bookokrat::parsing::html_to_markdown::script_as_text;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

#[derive(Serialize)]
pub struct NodeDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// For a block the book sets apart in a box: its place in the run of
    /// such blocks it stands in (`only`, `first`, `middle` or `last`), which
    /// the page draws as one box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout: Option<&'static str>,
    /// `center` or `end` where the book sets the block's lines that way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aligned: Option<&'static str>,
    #[serde(flatten)]
    pub block: BlockDto,
}

#[derive(Serialize)]
pub struct ListItemDto {
    /// What links to this entry point at, as endnotes in a list do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub blocks: Vec<NodeDto>,
}

#[derive(Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum BlockDto {
    H {
        level: u8,
        text: Vec<InlineDto>,
    },
    P {
        text: Vec<InlineDto>,
    },
    Code {
        #[serde(skip_serializing_if = "Option::is_none")]
        lang: Option<String>,
        code: String,
    },
    Quote {
        blocks: Vec<NodeDto>,
    },
    List {
        ordered: bool,
        start: u32,
        items: Vec<ListItemDto>,
    },
    Table {
        #[serde(skip_serializing_if = "Option::is_none")]
        header: Option<Vec<CellDto>>,
        rows: Vec<Vec<CellDto>>,
        align: Vec<&'static str>,
    },
    Dl {
        items: Vec<DefinitionDto>,
    },
    Epub {
        kind: String,
        el: String,
        blocks: Vec<NodeDto>,
    },
    Hr,
}

#[derive(Serialize)]
pub struct DefinitionDto {
    pub term: Vec<InlineDto>,
    pub defs: Vec<Vec<NodeDto>>,
}

#[derive(Serialize)]
pub struct CellDto {
    pub header: bool,
    pub rowspan: u32,
    pub colspan: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Vec<InlineDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<NodeDto>>,
}

#[derive(Serialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum InlineDto {
    Text {
        s: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        style: Option<&'static str>,
        /// `sup` or `sub` for text set above or below the line.
        #[serde(skip_serializing_if = "Option::is_none")]
        script: Option<&'static str>,
        /// For such text, how many characters the terminal app writes it
        /// with (`^{[4]}` for `[4]`): notes count positions that way.
        #[serde(skip_serializing_if = "Option::is_none")]
        chars: Option<usize>,
    },
    Link {
        kind: &'static str,
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        chapter: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
        /// Set when the link points at a file that continues a chapter
        /// rather than starting one: which of the chapter's files it is.
        #[serde(skip_serializing_if = "Option::is_none")]
        part: Option<usize>,
        children: Vec<InlineDto>,
    },
    Img {
        src: String,
        alt: String,
    },
    Anchor {
        id: String,
    },
    Br,
    Sbr,
}

/// Converts one of a chapter's files. `chapter_path` is the file's path
/// inside the book, used to turn its relative URLs into book-root paths;
/// `chapters` maps those paths to a chapter and the file's place within it,
/// so links arrive already resolved.
pub struct Converter<'a> {
    pub chapter_path: Option<&'a Path>,
    pub chapters: &'a HashMap<PathBuf, (usize, usize)>,
}

impl Converter<'_> {
    pub fn document(&self, doc: &Document) -> Vec<NodeDto> {
        self.nodes(&doc.blocks)
    }

    fn nodes(&self, nodes: &[Node]) -> Vec<NodeDto> {
        let set_off = |at: usize| nodes.get(at).is_some_and(|node| node.look.set_off);
        nodes
            .iter()
            .enumerate()
            .map(|(at, node)| {
                let mut dto = self.node(node);
                if node.look.set_off {
                    dto.callout = Some(match (at > 0 && set_off(at - 1), set_off(at + 1)) {
                        (false, false) => "only",
                        (false, true) => "first",
                        (true, true) => "middle",
                        (true, false) => "last",
                    });
                }
                dto
            })
            .collect()
    }

    fn node(&self, node: &Node) -> NodeDto {
        let block = match &node.block {
            Block::Heading { level, content } => BlockDto::H {
                level: level.as_u8(),
                text: self.text(content),
            },
            Block::Paragraph { content } => BlockDto::P {
                text: self.text(content),
            },
            Block::CodeBlock { language, content } => BlockDto::Code {
                lang: language.clone(),
                code: content.clone(),
            },
            Block::Quote { content } => BlockDto::Quote {
                blocks: self.nodes(content),
            },
            Block::List { kind, items } => {
                let (ordered, start) = match kind {
                    ListKind::Ordered { start } => (true, *start),
                    ListKind::Unordered => (false, 1),
                };
                BlockDto::List {
                    ordered,
                    start,
                    items: items
                        .iter()
                        .map(|i| ListItemDto {
                            id: i.id.clone(),
                            blocks: self.nodes(&i.content),
                        })
                        .collect(),
                }
            }
            Block::Table {
                header,
                rows,
                alignment,
            } => BlockDto::Table {
                header: header.as_ref().map(|r| self.row(r)),
                rows: rows.iter().map(|r| self.row(r)).collect(),
                align: alignment
                    .iter()
                    .map(|a| match a {
                        TableAlignment::None => "none",
                        TableAlignment::Left => "left",
                        TableAlignment::Center => "center",
                        TableAlignment::Right => "right",
                    })
                    .collect(),
            },
            Block::DefinitionList { items } => BlockDto::Dl {
                items: items
                    .iter()
                    .map(|i| DefinitionDto {
                        term: self.text(&i.term),
                        defs: i.definitions.iter().map(|d| self.nodes(d)).collect(),
                    })
                    .collect(),
            },
            Block::EpubBlock {
                epub_type,
                element_name,
                content,
            } => BlockDto::Epub {
                kind: epub_type.clone(),
                el: element_name.clone(),
                blocks: self.nodes(content),
            },
            Block::ThematicBreak => BlockDto::Hr,
        };
        NodeDto {
            id: node.id.clone(),
            callout: None,
            aligned: node.look.align.map(|align| match align {
                BlockAlign::Center => "center",
                BlockAlign::End => "end",
            }),
            block,
        }
    }

    fn row(&self, row: &TableRow) -> Vec<CellDto> {
        row.cells.iter().map(|c| self.cell(c)).collect()
    }

    fn cell(&self, cell: &TableCell) -> CellDto {
        let (text, blocks) = match &cell.content {
            TableCellContent::Simple(t) => (Some(self.text(t)), None),
            TableCellContent::Rich(nodes) => (None, Some(self.nodes(nodes))),
        };
        CellDto {
            header: cell.is_header,
            rowspan: cell.rowspan,
            colspan: cell.colspan,
            text,
            blocks,
        }
    }

    fn text(&self, text: &Text) -> Vec<InlineDto> {
        text.iter()
            .map(|item| match item {
                TextOrInline::Text(node) => InlineDto::Text {
                    s: node.content.clone(),
                    style: node.style.as_ref().map(|s| match s {
                        Style::Code => "code",
                        Style::Emphasis => "em",
                        Style::Strong => "strong",
                        Style::Strikethrough => "strike",
                    }),
                    script: node.script.map(|script| match script {
                        Script::Super => "sup",
                        Script::Sub => "sub",
                    }),
                    chars: node
                        .script
                        .map(|script| script_as_text(&node.content, script).chars().count()),
                },
                TextOrInline::Inline(inline) => self.inline(inline),
            })
            .collect()
    }

    fn inline(&self, inline: &Inline) -> InlineDto {
        match inline {
            Inline::Link {
                text,
                url,
                link_type,
                target_chapter,
                target_anchor,
                ..
            } => {
                let target = target_chapter
                    .as_deref()
                    .and_then(|href| self.chapters.get(&self.resolve_path(href)).copied());
                InlineDto::Link {
                    kind: match link_type {
                        LinkType::External => "external",
                        LinkType::InternalChapter => "chapter",
                        LinkType::InternalAnchor => "anchor",
                    },
                    url: url.clone(),
                    chapter: target.map(|(chapter, _)| chapter),
                    anchor: target_anchor.clone(),
                    part: target
                        .map(|(_, part)| part)
                        .filter(|part| *part > 0 && target_anchor.is_none()),
                    children: self.text(text),
                }
            }
            Inline::Image { alt_text, url, .. } => InlineDto::Img {
                src: self.image_source(url),
                alt: alt_text.clone(),
            },
            Inline::Anchor { id } => InlineDto::Anchor { id: id.clone() },
            Inline::LineBreak => InlineDto::Br,
            Inline::SoftBreak => InlineDto::Sbr,
        }
    }

    /// Pictures are files in the book, except inline SVG, which the core
    /// hands over as a `data:` URI. Many books wrap their cover (or a full-page
    /// plate) in an SVG holding nothing but an `<image>` of a file in the book;
    /// an SVG shown as a picture may not load files, so that one is shown as
    /// the file it points at.
    fn image_source(&self, url: &str) -> String {
        if !url.starts_with("data:") {
            return self.resolve_resource(url);
        }
        svg_wrapped_image(url)
            .map(|href| self.resolve_resource(&href))
            .unwrap_or_else(|| url.to_string())
    }

    fn resolve_resource(&self, url: &str) -> String {
        self.resolve_path(url).to_string_lossy().replace('\\', "/")
    }

    fn resolve_path(&self, url: &str) -> PathBuf {
        let url = url.split(['#', '?']).next().unwrap_or(url);
        let base = self
            .chapter_path
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new(""));
        normalize(&base.join(percent_decode(url)))
    }
}

pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(p) => out.push(p),
            _ => {}
        }
    }
    out
}

/// The file an inline SVG shows when all it holds is one `<image>` of a
/// file in the book.
fn svg_wrapped_image(data_uri: &str) -> Option<String> {
    use base64::Engine;
    let encoded = data_uri.strip_prefix("data:image/svg+xml;base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .inspect_err(|e| log::error!("Failed to decode an inline SVG: {e}"))
        .ok()?;
    let xml = String::from_utf8_lossy(&bytes);
    let svg = roxmltree::Document::parse(&xml)
        .inspect_err(|e| log::error!("Failed to parse an inline SVG: {e}"))
        .ok()?;
    let mut drawn = svg.descendants().filter(|node| {
        node.is_element()
            && !matches!(
                node.tag_name().name(),
                "svg" | "g" | "title" | "desc" | "metadata"
            )
    });
    let image = drawn.next()?;
    if image.tag_name().name() != "image" || drawn.next().is_some() {
        return None;
    }
    let href = image
        .attributes()
        .find(|attr| attr.name() == "href")?
        .value()
        .trim();
    let external = href.starts_with("data:") || href.contains("://");
    (!href.is_empty() && !external).then(|| href.to_string())
}

pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
