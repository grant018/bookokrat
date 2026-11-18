//! What a book's style sheets say about its text that a reader has to keep
//! for the text to read as written: words made bold, italic or struck
//! through by a class rather than a tag, paragraphs set apart in a box (pull
//! quotes, callouts) and centred or right-aligned lines. Fonts, sizes,
//! colours and spacing stay the reader's own.

use regex::Regex;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::LazyLock;

static LINK_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<link\b[^>]*>").unwrap());
static STYLE_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<style\b[^>]*>(.*?)</style\s*>").unwrap());
static REL_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\brel\s*=\s*["']([^"']*)["']"#).unwrap());
static HREF_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)\bhref\s*=\s*["']([^"']*)["']"#).unwrap());

/// A fill this close to white is the paper, not a box.
const PAPER_CHANNEL: f32 = 250.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// What the rules matching one element say; `None` where they say nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Declared {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub struck: Option<bool>,
    pub align: Option<Align>,
    pub filled: Option<bool>,
    pub ruled: Option<bool>,
}

impl Declared {
    pub fn overlaid_with(self, later: Declared) -> Declared {
        Declared {
            bold: later.bold.or(self.bold),
            italic: later.italic.or(self.italic),
            struck: later.struck.or(self.struck),
            align: later.align.or(self.align),
            filled: later.filled.or(self.filled),
            ruled: later.ruled.or(self.ruled),
        }
    }

    /// Whether the element stands in a box of its own: a fill behind it, or
    /// a rule down its side or around it.
    pub fn set_off(&self) -> bool {
        self.filled == Some(true) || self.ruled == Some(true)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

#[derive(Debug, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

impl Compound {
    fn matches(&self, element: &Element) -> bool {
        self.tag.as_ref().is_none_or(|tag| *tag == element.tag)
            && self
                .id
                .as_ref()
                .is_none_or(|id| element.id.as_ref() == Some(id))
            && self
                .classes
                .iter()
                .all(|class| element.classes.contains(class))
    }
}

#[derive(Debug)]
struct Rule {
    subject: Compound,
    /// The elements the subject must be inside, nearest first; `true` when
    /// the one below has to be its child rather than any descendant.
    above: Vec<(Compound, bool)>,
    specificity: (usize, usize, usize),
    declared: Declared,
}

/// The sources of a chapter's styles: the files it links, in order, and the
/// CSS written into the chapter itself.
#[derive(Debug, Default, PartialEq)]
pub struct Sources {
    pub linked: Vec<String>,
    pub embedded: Vec<String>,
}

pub fn sources(html: &str) -> Sources {
    let linked = LINK_TAG
        .find_iter(html)
        .filter_map(|tag| {
            let tag = tag.as_str();
            let rel = REL_ATTR
                .captures(tag)?
                .get(1)?
                .as_str()
                .to_ascii_lowercase();
            let sheet = rel.split_whitespace().any(|word| word == "stylesheet")
                && !rel.split_whitespace().any(|word| word == "alternate");
            let href = HREF_ATTR.captures(tag)?.get(1)?.as_str().trim();
            (sheet && !href.is_empty()).then(|| href.replace("&amp;", "&"))
        })
        .collect();
    let embedded = STYLE_TAG
        .captures_iter(html)
        .filter_map(|found| Some(found.get(1)?.as_str().to_string()))
        .filter(|css| !css.trim().is_empty())
        .collect();
    Sources { linked, embedded }
}

#[derive(Debug, Default)]
pub struct StyleSheet {
    rules: Vec<Rule>,
    by_id: HashMap<String, Vec<usize>>,
    by_class: HashMap<String, Vec<usize>>,
    by_tag: HashMap<String, Vec<usize>>,
    any: Vec<usize>,
}

impl StyleSheet {
    /// Adds a style sheet's rules after those already here. Rules inside
    /// `@media` and the like are left out, as are selectors this cannot
    /// weigh (pseudo-classes, attributes, siblings).
    pub fn add(&mut self, css: &str) {
        let css = without_comments(css);
        let mut rest = css.as_str();
        while let Some((prelude, body, after)) = next_rule(rest) {
            rest = after;
            let Some(body) = body else { continue };
            let prelude = prelude.trim();
            if prelude.starts_with('@') {
                continue;
            }
            let declared = declarations(body);
            if declared == Declared::default() {
                continue;
            }
            for selector in prelude.split(',') {
                if let Some((subject, above)) = parse_selector(selector) {
                    self.push(subject, above, declared);
                }
            }
        }
    }

    fn push(&mut self, subject: Compound, above: Vec<(Compound, bool)>, mut declared: Declared) {
        let parts = || std::iter::once(&subject).chain(above.iter().map(|(part, _)| part));
        let ids = parts().filter(|part| part.id.is_some()).count();
        let classes = parts().map(|part| part.classes.len()).sum();
        let tags = parts().filter(|part| part.tag.is_some()).count();
        // A fill or a rule on every paragraph is how the page looks, not a
        // way of setting some of them apart.
        if ids == 0 && classes == 0 {
            declared.filled = None;
            declared.ruled = None;
            if declared == Declared::default() {
                return;
            }
        }
        let index = self.rules.len();
        let bucket = if let Some(id) = &subject.id {
            self.by_id.entry(id.clone()).or_default()
        } else if let Some(class) = subject.classes.first() {
            self.by_class.entry(class.clone()).or_default()
        } else if let Some(tag) = &subject.tag {
            self.by_tag.entry(tag.clone()).or_default()
        } else {
            &mut self.any
        };
        bucket.push(index);
        self.rules.push(Rule {
            subject,
            above,
            specificity: (ids, classes, tags),
            declared,
        });
    }

    /// What the rules say about `element`. `ancestors` gives the elements it
    /// is inside, nearest first, and is only asked when a rule needs them.
    pub fn declared(
        &self,
        element: &Element,
        ancestors: impl FnOnce() -> Vec<Element>,
    ) -> Declared {
        let mut candidates: Vec<usize> = Vec::new();
        if let Some(found) = element.id.as_ref().and_then(|id| self.by_id.get(id)) {
            candidates.extend(found);
        }
        for class in &element.classes {
            if let Some(found) = self.by_class.get(class) {
                candidates.extend(found);
            }
        }
        if let Some(found) = self.by_tag.get(&element.tag) {
            candidates.extend(found);
        }
        candidates.extend(&self.any);
        if candidates.is_empty() {
            return Declared::default();
        }

        let chain = OnceCell::new();
        let mut ancestors = Some(ancestors);
        let mut matched: Vec<usize> = candidates
            .into_iter()
            .filter(|index| {
                let rule = &self.rules[*index];
                rule.subject.matches(element)
                    && (rule.above.is_empty()
                        || inside(
                            &rule.above,
                            chain.get_or_init(|| ancestors.take().map(|f| f()).unwrap_or_default()),
                        ))
            })
            .collect();
        matched.sort_unstable_by_key(|index| (self.rules[*index].specificity, *index));
        matched.dedup();
        matched
            .into_iter()
            .fold(Declared::default(), |so_far, index| {
                so_far.overlaid_with(self.rules[index].declared)
            })
    }
}

fn inside(above: &[(Compound, bool)], chain: &[Element]) -> bool {
    let Some(((part, parent), rest)) = above.split_first() else {
        return true;
    };
    if *parent {
        return chain
            .split_first()
            .is_some_and(|(element, further)| part.matches(element) && inside(rest, further));
    }
    (0..chain.len()).any(|at| part.matches(&chain[at]) && inside(rest, &chain[at + 1..]))
}

fn without_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        out.push(' ');
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// The next rule in `css`: what comes before its block, the block's inside
/// (`None` for a statement ending in `;`), and what follows it.
fn next_rule(css: &str) -> Option<(&str, Option<&str>, &str)> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut open = None;
    for (at, c) in css.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            ';' if depth == 0 => return Some((&css[..at], None, &css[at + 1..])),
            '{' => {
                if depth == 0 {
                    open = Some(at);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let open = open?;
                    return Some((&css[..open], Some(&css[open + 1..at]), &css[at + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits on `separator` where it stands outside quotes and brackets.
fn split_outside(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut start = 0;
    for (at, c) in text.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c == separator && depth == 0 => {
                parts.push(&text[start..at]);
                start = at + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Reads the inside of a rule's block, or a `style` attribute.
pub fn declarations(body: &str) -> Declared {
    let mut declared = Declared::default();
    for declaration in split_outside(body, ';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let value = value.to_ascii_lowercase();
        let value = value.split("!important").next().unwrap_or_default().trim();
        declare(
            &mut declared,
            name.trim().to_ascii_lowercase().as_str(),
            value,
        );
    }
    declared
}

fn declare(declared: &mut Declared, name: &str, value: &str) {
    let words = || value.split_whitespace();
    match name {
        "font-weight" => {
            if let Some(bold) = weight(value) {
                declared.bold = Some(bold);
            }
        }
        "font-style" => match words().next() {
            Some("italic" | "oblique") => declared.italic = Some(true),
            Some("normal") => declared.italic = Some(false),
            _ => {}
        },
        // The shorthand only ever adds here: `font: inherit` in a reset
        // sheet must not undo a <b>.
        "font" => {
            for word in words() {
                if matches!(word, "italic" | "oblique") {
                    declared.italic = Some(true);
                } else if word != "normal" && weight(word) == Some(true) {
                    declared.bold = Some(true);
                }
            }
        }
        "text-decoration" | "text-decoration-line" => {
            if words().any(|word| word == "line-through") {
                declared.struck = Some(true);
            }
        }
        "text-align" => match words().next() {
            Some("center") => declared.align = Some(Align::Center),
            Some("right" | "end") => declared.align = Some(Align::End),
            Some("left" | "start" | "justify") => declared.align = Some(Align::Start),
            _ => {}
        },
        "background-color" => {
            if let Some(filled) = fill(value, true) {
                declared.filled = Some(filled);
            }
        }
        "background" => {
            if let Some(filled) = fill(value, false) {
                declared.filled = Some(filled);
            }
        }
        "border"
        | "border-left"
        | "border-inline-start"
        | "border-style"
        | "border-left-style"
        | "border-inline-start-style" => {
            if words().any(|word| matches!(word, "none" | "hidden") || is_zero(word)) {
                declared.ruled = Some(false);
            } else if words().any(|word| {
                matches!(
                    word,
                    "solid"
                        | "dashed"
                        | "dotted"
                        | "double"
                        | "groove"
                        | "ridge"
                        | "inset"
                        | "outset"
                )
            }) {
                declared.ruled = Some(true);
            }
        }
        "border-width" | "border-left-width" | "border-inline-start-width"
            if words().all(is_zero) =>
        {
            declared.ruled = Some(false);
        }
        _ => {}
    }
}

fn weight(value: &str) -> Option<bool> {
    match value {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        _ => value.parse::<u32>().ok().map(|number| number >= 600),
    }
}

fn is_zero(word: &str) -> bool {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    !digits.is_empty() && digits.parse::<f32>().is_ok_and(|number| number == 0.0)
}

/// Whether a background value paints something a reader would see as a box.
/// `colour_only` is for `background-color`, where any other word is a
/// colour's name; in the `background` shorthand a word may be anything.
fn fill(value: &str, colour_only: bool) -> Option<bool> {
    let mut compact = String::with_capacity(value.len());
    let mut depth = 0usize;
    for c in value.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            c if c.is_whitespace() && depth > 0 => continue,
            _ => {}
        }
        compact.push(c);
    }
    for word in compact.split_whitespace() {
        match word {
            "transparent" | "none" => return Some(false),
            "inherit" | "initial" | "unset" | "revert" | "currentcolor" => return None,
            "white" => return Some(false),
            _ => {}
        }
        if let Some(hex) = word.strip_prefix('#') {
            return hex_channels(hex).map(visible);
        }
        if word.starts_with("rgb") {
            return function_channels(word).map(visible);
        }
        if word.starts_with("hsl") {
            return Some(true);
        }
        if colour_only && word.chars().all(|c| c.is_ascii_alphabetic()) {
            return Some(true);
        }
    }
    None
}

fn visible(channels: [f32; 4]) -> bool {
    let [red, green, blue, alpha] = channels;
    alpha > 0.0 && !(red >= PAPER_CHANNEL && green >= PAPER_CHANNEL && blue >= PAPER_CHANNEL)
}

fn hex_channels(hex: &str) -> Option<[f32; 4]> {
    let digits: Vec<u32> = hex.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
    let pair = |at: usize| (digits[at] * 16 + digits[at + 1]) as f32;
    let single = |at: usize| (digits[at] * 17) as f32;
    match digits.len() {
        3 => Some([single(0), single(1), single(2), 255.0]),
        4 => Some([single(0), single(1), single(2), single(3)]),
        6 => Some([pair(0), pair(2), pair(4), 255.0]),
        8 => Some([pair(0), pair(2), pair(4), pair(6)]),
        _ => None,
    }
}

fn function_channels(word: &str) -> Option<[f32; 4]> {
    let inner = word.split_once('(')?.1.trim_end_matches(')');
    let numbers: Vec<f32> = inner
        .split([',', '/'])
        .map(|part| {
            let part = part.trim();
            match part.strip_suffix('%') {
                Some(percent) => percent.parse::<f32>().ok().map(|p| p * 2.55),
                None => part.parse::<f32>().ok(),
            }
        })
        .collect::<Option<_>>()?;
    match numbers.as_slice() {
        [red, green, blue] => Some([*red, *green, *blue, 255.0]),
        [red, green, blue, alpha] => Some([*red, *green, *blue, *alpha]),
        _ => None,
    }
}

fn parse_selector(text: &str) -> Option<(Compound, Vec<(Compound, bool)>)> {
    let unweighable = |c: char| {
        matches!(
            c,
            ':' | '[' | ']' | '+' | '~' | '|' | '(' | ')' | '\\' | '"' | '\'' | '@' | '%' | '&'
        )
    };
    if text.contains(unweighable) {
        return None;
    }
    let spaced = text.replace('>', " > ");
    let mut parts: Vec<(Compound, bool)> = Vec::new();
    let mut child = false;
    for word in spaced.split_whitespace() {
        if word == ">" {
            if parts.is_empty() || child {
                return None;
            }
            child = true;
            continue;
        }
        parts.push((parse_compound(word)?, child));
        child = false;
    }
    if child {
        return None;
    }
    let (subject, mut parent) = parts.pop()?;
    let mut above = Vec::with_capacity(parts.len());
    while let Some((part, is_child)) = parts.pop() {
        above.push((part, parent));
        parent = is_child;
    }
    Some((subject, above))
}

fn parse_compound(word: &str) -> Option<Compound> {
    let mut compound = Compound::default();
    let mut rest = word;
    let name_end = |text: &str| {
        text.char_indices()
            .find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '-' | '_')))
            .map_or(text.len(), |(at, _)| at)
    };
    if let Some(after) = rest.strip_prefix('*') {
        rest = after;
    } else {
        let end = name_end(rest);
        if end > 0 {
            compound.tag = Some(rest[..end].to_ascii_lowercase());
            rest = &rest[end..];
        }
    }
    while let Some(mark) = rest.chars().next() {
        let after = &rest[mark.len_utf8()..];
        let end = name_end(after);
        if end == 0 {
            return None;
        }
        match mark {
            '.' => compound.classes.push(after[..end].to_string()),
            '#' => compound.id = Some(after[..end].to_string()),
            _ => return None,
        }
        rest = &after[end..];
    }
    Some(compound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(tag: &str, classes: &[&str]) -> Element {
        Element {
            tag: tag.to_string(),
            id: None,
            classes: classes.iter().map(|class| class.to_string()).collect(),
        }
    }

    fn sheet(css: &str) -> StyleSheet {
        let mut sheet = StyleSheet::default();
        sheet.add(css);
        sheet
    }

    fn alone(sheet: &StyleSheet, tag: &str, classes: &[&str]) -> Declared {
        sheet.declared(&element(tag, classes), Vec::new)
    }

    #[test]
    fn classes_carry_bold_italic_and_struck_text() {
        let sheet = sheet(
            "span.b { font-weight: bold } .i { font-style: italic; }
             .heavy { font-weight: 700 } .light { font-weight: 400 }
             .gone { text-decoration: line-through }",
        );
        assert_eq!(alone(&sheet, "span", &["b"]).bold, Some(true));
        assert_eq!(alone(&sheet, "p", &["b"]).bold, None);
        assert_eq!(alone(&sheet, "span", &["i"]).italic, Some(true));
        assert_eq!(alone(&sheet, "span", &["heavy"]).bold, Some(true));
        assert_eq!(alone(&sheet, "span", &["light"]).bold, Some(false));
        assert_eq!(alone(&sheet, "span", &["gone"]).struck, Some(true));
    }

    #[test]
    fn a_fill_or_a_side_rule_sets_a_paragraph_off() {
        let sheet = sheet(
            "/* pull quotes */
             p.callout, .aside { background-color: #EEE; border-left: 4px solid black; }
             p.paper { background-color: #fff }
             p.bare { border: 0 }
             p.lined { border-bottom: 1px solid black }",
        );
        assert!(alone(&sheet, "p", &["callout"]).set_off());
        assert!(alone(&sheet, "div", &["aside"]).set_off());
        assert!(!alone(&sheet, "p", &["paper"]).set_off());
        assert!(!alone(&sheet, "p", &["bare"]).set_off());
        assert!(!alone(&sheet, "p", &["lined"]).set_off());
    }

    #[test]
    fn a_fill_on_every_paragraph_sets_none_apart() {
        let sheet = sheet("p { background: #eee; text-align: center } * { border: 1px solid }");
        let declared = alone(&sheet, "p", &[]);
        assert!(!declared.set_off());
        assert_eq!(declared.align, Some(Align::Center));
    }

    #[test]
    fn the_more_specific_and_the_later_rule_win() {
        let sheet = sheet(
            "p.note { font-style: normal } .note { font-style: italic }
             .twice { text-align: center } .twice { text-align: right }",
        );
        assert_eq!(alone(&sheet, "p", &["note"]).italic, Some(false));
        assert_eq!(alone(&sheet, "p", &["twice"]).align, Some(Align::End));
    }

    #[test]
    fn rules_for_elements_inside_others_look_at_what_is_above() {
        let sheet = sheet("div.box p { font-style: italic } div.box > span { font-weight: bold }");
        let inside_box = || vec![element("section", &[]), element("div", &["box"])];
        let child_of_box = || vec![element("div", &["box"])];
        assert_eq!(
            sheet.declared(&element("p", &[]), inside_box).italic,
            Some(true)
        );
        assert_eq!(sheet.declared(&element("p", &[]), Vec::new).italic, None);
        assert_eq!(
            sheet.declared(&element("span", &[]), child_of_box).bold,
            Some(true)
        );
        assert_eq!(sheet.declared(&element("span", &[]), inside_box).bold, None);
    }

    #[test]
    fn at_rules_and_unweighable_selectors_are_left_out() {
        let sheet = sheet(
            "@font-face { font-family: \"M\"; src: url(a;b.ttf); font-weight: bold }
             @media amzn-mobi { p.k { font-weight: bold } }
             p.k:first-line { font-weight: bold }
             p.k + p.k { font-style: italic }
             p.k { text-align: center }",
        );
        assert_eq!(
            alone(&sheet, "p", &["k"]),
            Declared {
                align: Some(Align::Center),
                ..Declared::default()
            }
        );
    }

    #[test]
    fn a_reset_does_not_undo_bold() {
        assert_eq!(
            declarations("font: inherit; font-weight: inherit"),
            Declared::default()
        );
        assert_eq!(
            declarations("font: italic bold 1em/1.2 serif").bold,
            Some(true)
        );
    }

    #[test]
    fn a_chapter_names_its_style_sheets() {
        let found = sources(
            r#"<head><link href="css/style.css" rel="stylesheet" type="text/css"/>
               <link rel="icon" href="x.png"/>
               <style type="text/css">p.q { font-style: italic }</style></head>"#,
        );
        assert_eq!(found.linked, vec!["css/style.css".to_string()]);
        assert_eq!(
            found.embedded,
            vec!["p.q { font-style: italic }".to_string()]
        );
    }
}
