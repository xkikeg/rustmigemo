use std::char::{REPLACEMENT_CHARACTER, decode_utf16};

#[derive(Debug)]
pub struct RegexNode {
    pub code: char,
    pub child: Option<Box<RegexNode>>,
    pub next: Option<Box<RegexNode>>,
}

pub const DEFAULT_ESCAPE_CHARS: &str = r"\.[]{}()*+?^$|";
pub const VIM_ESCAPE_CHARS: &str = r"\.[]*~^$/";
pub const EMACS_ESCAPE_CHARS: &str = r"\.[]*+?^$";

pub enum RegexOperator {
    Default,
    Vim,
    Emacs,
    VimNonNewline,
    EmacsNonNewline,
    User {
        or: String,
        begin_group: String,
        end_group: String,
        begin_class: String,
        end_class: String,
        newline: String,
        escape: String,
    },
}

#[derive(Debug, Clone)]
pub struct RegexOperatorDetail {
    pub or: String,
    pub begin_group: String,
    pub end_group: String,
    pub begin_class: String,
    pub end_class: String,
    pub newline: String,
    pub escape: String,
    escape_in_class: bool,
    escape_ascii: [u64; 2],
    has_non_ascii_escape: bool,
}

impl RegexOperatorDetail {
    fn build(
        or: String,
        begin_group: String,
        end_group: String,
        begin_class: String,
        end_class: String,
        newline: String,
        escape: String,
        escape_in_class: bool,
    ) -> RegexOperatorDetail {
        let mut escape_ascii = [0u64; 2];
        let mut has_non_ascii_escape = false;
        for c in escape.chars() {
            if c < '\u{80}' {
                escape_ascii[(c as usize) >> 6] |= 1u64 << ((c as u32) & 63);
            } else {
                has_non_ascii_escape = true;
            }
        }
        RegexOperatorDetail {
            or,
            begin_group,
            end_group,
            begin_class,
            end_class,
            newline,
            escape,
            escape_in_class,
            escape_ascii,
            has_non_ascii_escape,
        }
    }

    #[inline]
    pub fn needs_escape(&self, c: char) -> bool {
        if c < '\u{80}' {
            ((self.escape_ascii[(c as usize) >> 6] >> ((c as u32) & 63)) & 1) == 1
        } else if self.has_non_ascii_escape {
            self.escape.contains(c)
        } else {
            false
        }
    }

    #[inline]
    pub(crate) fn write_escaped_char(&self, c: char, buf: &mut String) {
        if self.needs_escape(c) {
            buf.push('\\');
        }
        buf.push(c);
    }

    /// Writes a character class (`[...]`) containing `chars` without introducing
    /// unintended ranges, negation, or engine-specific backslash pitfalls.
    pub(crate) fn write_char_class(&self, chars: impl Iterator<Item = char>, buf: &mut String) {
        let mut has_rbracket = false;
        let mut has_caret = false;
        let mut has_dash = false;
        let mut others = Vec::new();
        for c in chars {
            match c {
                ']' => has_rbracket = true,
                '^' => has_caret = true,
                '-' => has_dash = true,
                _ => others.push(c),
            }
        }

        buf.push_str(&self.begin_class);
        if has_rbracket {
            if self.escape_in_class {
                buf.push('\\');
            }
            buf.push(']');
        }
        for c in &others {
            if self.escape_in_class && *c == '\\' {
                buf.push('\\');
            }
            buf.push(*c);
        }
        if has_dash && !has_rbracket && others.is_empty() {
            // Only '-' and '^' are in the class; emit '-' first so '^' does not negate.
            buf.push('-');
            if has_caret {
                buf.push('^');
            }
        } else {
            if has_caret {
                buf.push('^');
            }
            if has_dash {
                buf.push('-');
            }
        }
        buf.push_str(&self.end_class);
    }

    pub fn get_regex_operator_detail(rxop: &RegexOperator) -> RegexOperatorDetail {
        match rxop {
            RegexOperator::Default => RegexOperatorDetail::build(
                "|".to_string(),
                "(".to_string(),
                ")".to_string(),
                "[".to_string(),
                "]".to_string(),
                String::new(),
                DEFAULT_ESCAPE_CHARS.to_string(),
                true,
            ),
            RegexOperator::Vim => RegexOperatorDetail::build(
                "\\|".to_string(),
                "\\%(".to_string(),
                "\\)".to_string(),
                "[".to_string(),
                "]".to_string(),
                "\\_s*".to_string(),
                VIM_ESCAPE_CHARS.to_string(),
                true,
            ),
            RegexOperator::VimNonNewline => RegexOperatorDetail::build(
                "\\|".to_string(),
                "\\%(".to_string(),
                "\\)".to_string(),
                "[".to_string(),
                "]".to_string(),
                String::new(),
                VIM_ESCAPE_CHARS.to_string(),
                true,
            ),
            RegexOperator::Emacs => RegexOperatorDetail::build(
                "\\|".to_string(),
                "\\(".to_string(),
                "\\)".to_string(),
                "[".to_string(),
                "]".to_string(),
                "\\s-*".to_string(),
                EMACS_ESCAPE_CHARS.to_string(),
                false,
            ),
            RegexOperator::EmacsNonNewline => RegexOperatorDetail::build(
                "\\|".to_string(),
                "\\(".to_string(),
                "\\)".to_string(),
                "[".to_string(),
                "]".to_string(),
                String::new(),
                EMACS_ESCAPE_CHARS.to_string(),
                false,
            ),
            RegexOperator::User {
                or,
                begin_group,
                end_group,
                begin_class,
                end_class,
                newline,
                escape,
            } => RegexOperatorDetail::build(
                or.clone(),
                begin_group.clone(),
                end_group.clone(),
                begin_class.clone(),
                end_class.clone(),
                newline.clone(),
                escape.clone(),
                true,
            ),
        }
    }
}

/// 正規表現ジェネレータの共通トレイト
pub trait RegexGeneratorTrait {
    fn add(&mut self, word: &[char]);
    fn generate(&self, operator: &RegexOperator) -> String;
}

#[derive(Debug)]
pub struct RegexGenerator {
    pub root: Option<Box<RegexNode>>,
}

impl RegexGenerator {
    pub fn add_utf16(&mut self, word: &[u16]) {
        if word.len() == 0 {
            return;
        }
        let utf32word: Vec<char> = decode_utf16(word.iter().cloned())
            .map(|r| r.unwrap_or(REPLACEMENT_CHARACTER))
            .collect();
        self.root = RegexGenerator::_add(::std::mem::replace(&mut self.root, None), &utf32word, 0);
    }

    fn _add(node: Option<Box<RegexNode>>, word: &[char], offset: usize) -> Option<Box<RegexNode>> {
        if node.is_none() {
            if offset >= word.len() {
                return None;
            }
            let child = if offset < word.len() - 1 {
                RegexGenerator::_add(None, word, offset + 1)
            } else {
                None
            };
            return Some(Box::new(RegexNode {
                code: word[offset],
                child: child,
                next: None,
            }));
        }
        let code = word[offset];
        // 最初のノードがcodeより大きければ、リストの先頭に挿入
        if code < node.as_ref().unwrap().code {
            return Some(Box::new(RegexNode {
                code: code,
                child: RegexGenerator::_add(None, word, offset + 1),
                next: node,
            }));
        } else {
            fn find_le_node(node: &mut Box<RegexNode>, code: char) -> &mut Box<RegexNode> {
                if node.next.is_some() && node.next.as_ref().unwrap().code <= code {
                    if let Some(ref mut _n) = node.next {
                        return find_le_node(_n, code);
                    } else {
                        panic!()
                    }
                } else {
                    return node;
                }
            }
            let mut _node = node.unwrap();
            let another_node = find_le_node(&mut _node, code);
            if another_node.as_ref().code == code {
                if another_node.as_ref().child.is_none() {
                    return Some(_node);
                } else {
                    if word.len() == offset + 1 {
                        another_node.child = None;
                    } else {
                        another_node.child =
                            RegexGenerator::_add(::std::mem::replace(&mut another_node.child, None), word, offset + 1)
                    }
                    return Some(_node);
                }
            } else {
                let mut new_node = Box::new(RegexNode {
                    code: code,
                    child: None,
                    next: ::std::mem::replace(&mut another_node.next, None),
                });
                new_node.child = if offset + 1 == word.len() {
                    None
                } else {
                    RegexGenerator::_add(None, word, offset + 1)
                };
                another_node.next = Some(new_node);
                return Some(_node);
            }
        }
    }

    pub fn generate(&self, operator: &RegexOperator) -> String {
        return match &self.root {
            Some(_) => {
                let mut string: String = String::new();
                let operator_detail = RegexOperatorDetail::get_regex_operator_detail(operator);
                self.generate_stub(&self.root, &operator_detail, &mut string);
                string
            }
            None => "".to_string(),
        };
    }

    fn generate_stub(&self, node: &Option<Box<RegexNode>>, operator: &RegexOperatorDetail, buf: &mut String) {
        let mut brother = 1;
        let mut haschild = 0;
        let mut tmp = node;
        while tmp.is_some() {
            let tmp_unwrap = tmp.as_ref().unwrap();
            if tmp_unwrap.next.is_some() {
                brother = brother + 1;
            }
            if tmp_unwrap.child.is_some() {
                haschild = haschild + 1;
            }
            tmp = &tmp_unwrap.next;
        }
        let nochild = brother - haschild;

        if brother > 1 && haschild > 0 {
            buf.push_str(&operator.begin_group);
        }
        if nochild > 0 {
            if nochild > 1 {
                let mut chars = Vec::with_capacity(nochild);
                let mut tmp = node;
                while let Some(n) = tmp.as_ref() {
                    if n.child.is_none() {
                        chars.push(n.code);
                    }
                    tmp = &n.next;
                }
                operator.write_char_class(chars.into_iter(), buf);
            } else {
                let mut tmp = node;
                while let Some(n) = tmp.as_ref() {
                    if n.child.is_none() {
                        operator.write_escaped_char(n.code, buf);
                        break;
                    }
                    tmp = &n.next;
                }
            }
        }

        if haschild > 0 {
            if nochild > 0 {
                buf.push_str(&operator.or);
            }
            let mut tmp = node;
            while tmp.as_ref().unwrap().child.is_none() {
                tmp = &tmp.as_ref().unwrap().next;
            }
            loop {
                operator.write_escaped_char(tmp.as_ref().unwrap().code, buf);
                if operator.newline.len() > 0 {
                    buf.push_str(&operator.newline);
                }
                self.generate_stub(&tmp.as_ref().unwrap().child, operator, buf);
                tmp = &tmp.as_ref().unwrap().next;
                while tmp.is_some() && tmp.as_ref().unwrap().child.is_none() {
                    tmp = &tmp.as_ref().unwrap().next;
                }
                if tmp.is_none() {
                    break;
                }
                if haschild > 1 {
                    buf.push_str(&operator.or);
                }
            }
        }
        if brother > 1 && haschild > 0 {
            buf.push_str(&operator.end_group);
        }
    }
}

impl RegexGeneratorTrait for RegexGenerator {
    fn add(&mut self, word: &[char]) {
        if word.len() == 0 {
            return;
        }
        self.root = RegexGenerator::_add(::std::mem::replace(&mut self.root, None), word, 0);
    }

    fn generate(&self, operator: &RegexOperator) -> String {
        return match &self.root {
            Some(_) => {
                let mut string: String = String::new();
                let operator_detail = RegexOperatorDetail::get_regex_operator_detail(operator);
                self.generate_stub(&self.root, &operator_detail, &mut string);
                string
            }
            None => "".to_string(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_dad() {
        let mut rxgen = RegexGenerator { root: None };
        let rxop = RegexOperator::Default;
        let bad: Vec<char> = "bad".chars().collect();
        let dad: Vec<char> = "dad".chars().collect();
        rxgen.add(&bad);
        rxgen.add(&dad);
        let actual = rxgen.generate(&rxop);
        let expected = "(bad|dad)";
        assert_eq!(actual, expected);
    }

    #[test]
    fn dad_bat() {
        let mut rxgen = RegexGenerator { root: None };
        let rxop = RegexOperator::Default;
        let bad: Vec<char> = "bad".chars().collect();
        let bat: Vec<char> = "bat".chars().collect();
        rxgen.add(&bad);
        rxgen.add(&bat);
        let actual = rxgen.generate(&rxop);
        let expected = "ba[dt]";
        assert_eq!(actual, expected);
    }

    #[test]
    fn a_b_a() {
        let mut rxgen = RegexGenerator { root: None };
        let rxop = RegexOperator::Default;
        let a1: Vec<char> = "a".chars().collect();
        let b: Vec<char> = "b".chars().collect();
        let a2: Vec<char> = "a".chars().collect();
        rxgen.add(&a1);
        rxgen.add(&b);
        rxgen.add(&a2);
        let actual = rxgen.generate(&rxop);
        let expected = "[ab]";
        assert_eq!(actual, expected);
    }

    #[test]
    fn escape() {
        let mut rxgen = RegexGenerator { root: None };
        let rxop = RegexOperator::Default;
        let a_b: Vec<char> = "a.b".chars().collect();
        rxgen.add(&a_b);
        let actual = rxgen.generate(&rxop);
        let expected = "a\\.b";
        assert_eq!(actual, expected);
    }

    #[test]
    fn surrogate_pair() {
        let mut rxgen = RegexGenerator { root: None };
        let rxop = RegexOperator::Default;
        let a: Vec<char> = "𠮟".chars().collect();
        let b: Vec<char> = "𠮷".chars().collect();
        rxgen.add(&a);
        rxgen.add(&b);
        let actual = rxgen.generate(&rxop);
        let expected = "[𠮟𠮷]";
        assert_eq!(actual, expected);
    }
}
