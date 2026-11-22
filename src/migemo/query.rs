use super::character_converter::*;
use super::compact_dictionary::*;
use super::regex_generator::*;
use super::romaji_processor::RomajiProcessor;
use super::ternary_regex_generator::*;
use std::char::{REPLACEMENT_CHARACTER, decode_utf16};
use std::iter::Peekable;
use std::str::CharIndices;
use std::sync::OnceLock;

fn romaji_processor() -> &'static RomajiProcessor {
    static ROMAJI_PROCESSOR: OnceLock<RomajiProcessor> = OnceLock::new();
    ROMAJI_PROCESSOR.get_or_init(RomajiProcessor::new)
}

pub fn query_a_word(word: &str, dict: &CompactDictionary, operator: &RegexOperator) -> String {
    query_a_word_with_generator(word, dict, operator, &mut TernaryRegexGenerator::new())
}

pub fn query_a_word_with_generator<T: RegexGeneratorTrait>(
    word: &str,
    dict: &CompactDictionary,
    operator: &RegexOperator,
    generator: &mut T,
) -> String {
    let word_chars: Vec<char> = word.chars().collect();
    generator.add(&word_chars);

    let lower: Vec<u16> = word.to_lowercase().encode_utf16().collect();
    for elem in dict.predictive_search(&lower) {
        let elem_chars: Vec<char> = decode_utf16(elem.iter().cloned())
            .map(|r| r.unwrap_or(REPLACEMENT_CHARACTER))
            .collect();
        generator.add(&elem_chars);
    }

    let zen_str = han2zen(word.to_string());
    let zen_chars: Vec<char> = zen_str.chars().collect();
    generator.add(&zen_chars);

    let han_str = zen2han(word.to_string());
    let han_chars: Vec<char> = han_str.chars().collect();
    generator.add(&han_chars);

    let hiragana = romaji_processor().romaji_to_hiragana_predictively(&lower);
    for suffix in hiragana.suffixes {
        let mut hira = hiragana.prefix.clone();
        hira.extend(suffix);
        let hira_chars: Vec<char> = decode_utf16(hira.iter().cloned())
            .map(|r| r.unwrap_or(REPLACEMENT_CHARACTER))
            .collect();
        generator.add(&hira_chars);

        for elem in dict.predictive_search(&hira) {
            let elem_chars: Vec<char> = decode_utf16(elem.iter().cloned())
                .map(|r| r.unwrap_or(REPLACEMENT_CHARACTER))
                .collect();
            generator.add(&elem_chars);
        }

        let kata = hira2kata(&String::from_utf16_lossy(&hira));
        let kata_chars: Vec<char> = kata.chars().collect();
        generator.add(&kata_chars);

        let zen_kata = zen2han(kata);
        let zen_kata_chars: Vec<char> = zen_kata.chars().collect();
        generator.add(&zen_kata_chars);
    }
    let generated = generator.generate(&operator);
    return generated;
}

pub fn query(word: String, dict: &CompactDictionary, operator: &RegexOperator) -> String {
    if word.is_empty() {
        return "".to_string();
    }
    let mut result = String::new();
    for w in tokenize(&word) {
        result.push_str(&query_a_word(w, dict, operator));
    }
    result
}

/// クエリ文字列をトークンに分割するイテレータ
pub struct TokenizeIter<'a> {
    // 元の文字列全体への参照
    full_str: &'a str,
    // 文字とバイトインデックスを扱うためのイテレータ
    indices: Peekable<CharIndices<'a>>,
}

impl<'a> TokenizeIter<'a> {
    /// 新しいイテレータを作成します。
    fn new(input: &'a str) -> Self {
        TokenizeIter {
            full_str: input,
            indices: input.char_indices().peekable(),
        }
    }
}

impl<'a> Iterator for TokenizeIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        // 1. トークンの開始位置を探す（先行する空白をスキップ）
        let start_index = match self.indices.find(|&(_, c)| !c.is_whitespace()) {
            Some((i, _)) => i,
            None => return None, // 文字列の終端に達したら終了
        };

        // 2. トークンの終了位置を探す
        // peek()で次の文字を「覗き見」しながら、トークンの区切りを判断する
        while let Some(&(current_index, current_char)) = self.indices.peek() {
            // 区切り文字（空白）が見つかったら、そこでトークンは終了
            if current_char.is_whitespace() {
                break;
            }

            // 前の文字の種類を取得
            // start_indexからcurrent_indexまでの部分文字列の最後の文字
            let prev_char = self.full_str[..current_index].chars().last().unwrap_or(' ');

            // トークンの区切りルールの判定
            // [修正] is_ascii_alphabetic から is_ascii_alphanumeric に変更し、数字もトークンに含めるようにした
            let is_boundary =
                // ルール1: 小文字の後に大文字が来た場合 (例: "toukyouO"saka)
                (prev_char.is_lowercase() && current_char.is_uppercase()) ||
                // ルール2: 2文字以上の大文字の後に小文字が来た場合 (例: "FUKUOKAh"okkaido)
                (prev_char.is_uppercase() && current_char.is_lowercase() && {
                    // "FUKUOKA" のように大文字が続いているか確認
                    let token_so_far = &self.full_str[start_index..current_index];
                    token_so_far.chars().count() > 1 && token_so_far.chars().all(|c| c.is_uppercase())
                }) ||
                // ルール3: 非ASCII英数字の後にASCII英数字が来た場合 (例: "東京T"ower, "東京123")
                (!prev_char.is_ascii_alphanumeric() && current_char.is_ascii_alphanumeric()) ||
                // ルール4: ASCII英数字の後に非ASCII英数字が来た場合 (例: "Tower東京", "word1東京")
                (prev_char.is_ascii_alphanumeric() && !current_char.is_ascii_alphanumeric());

            if is_boundary {
                break; // 区切りなのでループを抜ける
            }

            // 区切りでなければ、イテレータを1つ進める
            self.indices.next();
        }

        // 3. トークンを切り出して返す
        let end_index = self.indices.peek().map_or(self.full_str.len(), |&(i, _)| i);
        Some(&self.full_str[start_index..end_index])
    }
}

pub fn tokenize(input: &str) -> TokenizeIter<'_> {
    TokenizeIter::new(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_with_both_generators(words: &[&str], op: &RegexOperator) -> (String, String) {
        let mut gen1 = RegexGenerator { root: None };
        let mut gen2 = TernaryRegexGenerator::new();

        for word in words {
            let chars: Vec<char> = word.chars().collect();
            gen1.add(&chars);
            gen2.add(&chars);
        }

        (gen1.generate(op), gen2.generate(op))
    }

    #[test]
    fn test_tokenize() {
        let query = "toukyouOosaka nagoyaFUKUOKAhokkaido ";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert_eq!(tokens, vec!["toukyou", "Oosaka", "nagoya", "FUKUOKA", "hokkaido"]);
    }
    #[test]
    fn test_tokenize_1() {
        let query = "a";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert_eq!(tokens, vec!["a"]);
    }

    #[test]
    fn test_tokenize_2() {
        let query = "A";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert_eq!(tokens, vec!["A"]);
    }

    #[test]
    fn test_tokenize_3() {
        let query = "あ";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert_eq!(tokens, vec!["あ"]);
    }

    #[test]
    fn test_tokenize_4() {
        let query = "/";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert_eq!(tokens, vec!["/"]);
    }

    #[test]
    fn test_tokenize_5() {
        let query = "aaA";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert_eq!(tokens, vec!["aa", "A"]);
    }

    #[test]
    fn test_tokenize_empty() {
        let query = "";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_tokenize_whitespace() {
        let query = "  word1   word2 ";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert_eq!(tokens, vec!["word1", "word2"]);
    }

    #[test]
    fn test_tokenize_mixed() {
        let query = "東京Tower";
        let tokens: Vec<&str> = tokenize(query).collect();
        assert_eq!(tokens, vec!["東京", "Tower"]);
    }

    #[test]
    fn test_generator_compatibility() {
        // 改行演算子あり/なしを含め、両ジェネレータの互換性を確認
        let (default1, default2) = generate_with_both_generators(&["test"], &RegexOperator::Default);
        assert_eq!(default1, default2);
        assert_eq!(default1, "test");

        let (vim1, vim2) = generate_with_both_generators(&["test"], &RegexOperator::Vim);
        assert_eq!(vim1, vim2);
        assert!(vim2.contains("\\_s*"));

        let (emacs1, emacs2) = generate_with_both_generators(&["test"], &RegexOperator::Emacs);
        assert_eq!(emacs1, emacs2);
        assert!(emacs2.contains("\\s-*"));
        assert!(!emacs2.contains("\\_s-*"));

        let user_op = RegexOperator::User {
            or: "|".to_string(),
            begin_group: "(".to_string(),
            end_group: ")".to_string(),
            begin_class: "[".to_string(),
            end_class: "]".to_string(),
            newline: "<NL>".to_string(),
            escape: DEFAULT_ESCAPE_CHARS.to_string(),
        };
        let (user1, user2) = generate_with_both_generators(&["bad", "bat"], &user_op);
        assert_eq!(user1, user2);
        assert!(user2.contains("<NL>"));

        // Special characters inside and outside character classes must match across generators.
        let special_cases: &[&[&str]] = &[
            &["A+", "A-", "Aー"],
            &["a-", "a^"],
            &["a]", "a\\"],
            &["a]", "a\\", "a^", "a-"],
            &["f(x)", "f(", "a+b", "a?b", "a{1}", "a|b", "a~b", "a/b"],
            &["a(", "a)", "a+", "a?", "a{", "a}", "a|"],
            &["ab", "a"],
            &["b", "a"],
        ];
        let operators = [
            RegexOperator::Default,
            RegexOperator::Vim,
            RegexOperator::VimNonNewline,
            RegexOperator::Emacs,
            RegexOperator::EmacsNonNewline,
        ];
        for op in &operators {
            for words in special_cases {
                let (g1, g2) = generate_with_both_generators(words, op);
                assert_eq!(g1, g2, "mismatch for words={:?}", words);
            }
        }
    }

    #[test]
    fn test_escape_per_mode() {
        // Default (ERE/PCRE): escapes ( ) { } + ? | . * [ ] ^ $ \ outside classes
        let (_, p) = generate_with_both_generators(&["f(x)"], &RegexOperator::Default);
        assert_eq!(p, r"f\(x\)");
        let (_, p) = generate_with_both_generators(&["a+b?c{1}|d~e/f-g"], &RegexOperator::Default);
        assert_eq!(p, r"a\+b\?c\{1\}\|d~e/f-g");

        // Vim (magic): does NOT escape ( ) | { } + ?, DOES escape \ . [ ] * ~ ^ $ /
        let (_, p) = generate_with_both_generators(&["f(x)"], &RegexOperator::VimNonNewline);
        assert_eq!(p, "f(x)");
        let (_, p) = generate_with_both_generators(&["a+b?c{1}|d~e/f*g.h"], &RegexOperator::VimNonNewline);
        assert_eq!(p, r"a+b?c{1}|d\~e\/f\*g\.h");

        // Emacs: does NOT escape ( ) | { }, DOES escape \ . [ ] * + ? ^ $
        let (_, p) = generate_with_both_generators(&["f(x)"], &RegexOperator::EmacsNonNewline);
        assert_eq!(p, "f(x)");
        let (_, p) = generate_with_both_generators(&["f("], &RegexOperator::EmacsNonNewline);
        assert_eq!(p, "f(");
        let (_, p) = generate_with_both_generators(&["a+b?c{1}|d~e/f*g.h"], &RegexOperator::EmacsNonNewline);
        assert_eq!(p, r"a\+b\?c{1}|d~e/f\*g\.h");

        // User-configurable escape set (e.g., cmigemo-style)
        let custom = RegexOperator::User {
            or: "|".to_string(),
            begin_group: "(".to_string(),
            end_group: ")".to_string(),
            begin_class: "[".to_string(),
            end_class: "]".to_string(),
            newline: String::new(),
            escape: r"\.*+^$/".to_string(),
        };
        let (_, p) = generate_with_both_generators(&["f(x)/y"], &custom);
        assert_eq!(p, r"f(x)\/y");
    }

    #[test]
    fn test_char_class_escaping() {
        // '-' must not create a range ('+'..'ー')
        for op in [
            RegexOperator::Default,
            RegexOperator::VimNonNewline,
            RegexOperator::EmacsNonNewline,
        ] {
            let (g1, g2) = generate_with_both_generators(&["A+", "A-", "Aー"], &op);
            assert_eq!(g1, "A[+ー-]");
            assert_eq!(g2, "A[+ー-]");

            // '-' and '^' alone: '-' first so '^' does not negate
            let (g1, g2) = generate_with_both_generators(&["a-", "a^"], &op);
            assert_eq!(g1, "a[-^]");
            assert_eq!(g2, "a[-^]");

            // Operators like ( ) + ? { inside [...] need no backslashes
            let (g1, g2) = generate_with_both_generators(&["a(", "a)", "a+", "a?", "a{"], &op);
            assert_eq!(g1, "a[()+?{]");
            assert_eq!(g2, "a[()+?{]");
        }

        // ']' and '\' inside [...]: escaped in Default/Vim, unescaped with ']' first in Emacs
        let (_, def) = generate_with_both_generators(&["a]", "a\\", "a^", "a-"], &RegexOperator::Default);
        assert_eq!(def, r"a[\]\\^-]");
        let (_, vim) = generate_with_both_generators(&["a]", "a\\", "a^", "a-"], &RegexOperator::VimNonNewline);
        assert_eq!(vim, r"a[\]\\^-]");
        let (_, emacs) = generate_with_both_generators(&["a]", "a\\", "a^", "a-"], &RegexOperator::EmacsNonNewline);
        assert_eq!(emacs, r"a[]\^-]");
    }

    #[test]
    fn test_generator_compatibility_random() {
        use rand::rngs::StdRng;
        use rand::{RngExt, SeedableRng};

        const ALPHABET: &[char] = &[
            'a', 'b', 'c', 'A', '-', ']', '[', '^', '\\', '(', ')', '{', '}', '+', '*', '?', '.', '|', '~', '/', '$',
            'あ', 'ー',
        ];
        let operators = [
            RegexOperator::Default,
            RegexOperator::Vim,
            RegexOperator::VimNonNewline,
            RegexOperator::Emacs,
            RegexOperator::EmacsNonNewline,
        ];
        let mut rng = StdRng::seed_from_u64(0x5EED_C0DE);

        for _ in 0..200 {
            let word_count = rng.random_range(1..=12);
            let words: Vec<String> = (0..word_count)
                .map(|_| {
                    let len = rng.random_range(1..=5);
                    (0..len)
                        .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())])
                        .collect()
                })
                .collect();
            let refs: Vec<&str> = words.iter().map(String::as_str).collect();
            for op in &operators {
                let (g1, g2) = generate_with_both_generators(&refs, op);
                assert_eq!(g1, g2, "words={:?}", refs);
            }
        }
    }

    #[test]
    fn test_emacs_regexp_valid_in_emacs() {
        use std::process::Command;

        fn elisp_str(s: &str) -> String {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    _ => out.push(c),
                }
            }
            out.push('"');
            out
        }

        let (_, nl_pattern) = generate_with_both_generators(&["test", "text"], &RegexOperator::Emacs);
        let cases: &[(&[&str], &[&str], &[&str])] = &[
            (&["f(x)"], &["f(x)"], &["fx"]),
            (&["f("], &["f("], &["f"]),
            (&["a+b?c{1}|d~e/f*g.h"], &["a+b?c{1}|d~e/f*g.h"], &["ab?c{1}|d~e/f*gxh"]),
            (&["A+", "A-", "Aー"], &["A+", "A-", "Aー"], &["A0", "A\\"]),
            (&["a-", "a^"], &["a-", "a^"], &["ab"]),
            (&["a]", "a\\", "a^", "a-"], &["a]", "a\\", "a^", "a-"], &["ab"]),
            (
                &["a(", "a)", "a+", "a?", "a{"],
                &["a(", "a)", "a+", "a?", "a{"],
                &["a\\"],
            ),
        ];

        let mut pairs = String::from("(list");
        for op in [RegexOperator::Emacs, RegexOperator::EmacsNonNewline] {
            for (words, should_match, should_not_match) in cases {
                let (_, re) = generate_with_both_generators(words, &op);
                let anchored = format!("\\`{}\\'", re);
                for s in *should_match {
                    pairs.push_str(&format!(" (list {} {} t)", elisp_str(&anchored), elisp_str(s)));
                }
                for s in *should_not_match {
                    pairs.push_str(&format!(" (list {} {} nil)", elisp_str(&anchored), elisp_str(s)));
                }
            }
        }
        pairs.push(')');

        let elisp = format!(
            r#"(progn
              (let ((re {nl}))
                (unless (and (string-match-p re "test")
                             (string-match-p re "te st")
                             (string-match-p re "text")
                             (with-temp-buffer (text-mode) (string-match-p re "te\nst"))
                             (not (string-match-p re "tent")))
                  (error "unexpected newline match result for %S" re)))
              (dolist (tc {pairs})
                (let* ((re (nth 0 tc)) (s (nth 1 tc)) (exp (nth 2 tc))
                       (got (if (string-match-p re s) t nil)))
                  (unless (eq got exp)
                    (error "re=%S str=%S exp=%S got=%S" re s exp got)))))"#,
            nl = elisp_str(&nl_pattern),
            pairs = pairs,
        );

        match Command::new("emacs")
            .args(["--quick", "--batch", "--eval", &elisp])
            .output()
        {
            Ok(out) => {
                assert!(
                    out.status.success(),
                    "emacs regexp verification failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Skip if GNU Emacs is not installed in the test environment.
            }
            Err(e) => panic!("failed to invoke emacs: {}", e),
        }
    }

    #[test]
    fn test_vim_regexp_valid_in_vim() {
        use std::process::Command;

        fn vim_str(s: &str) -> String {
            format!("'{}'", s.replace('\'', "''"))
        }

        let cases: &[(&[&str], &[&str], &[&str])] = &[
            (&["f(x)"], &["f(x)"], &["fx"]),
            (&["f("], &["f("], &["f"]),
            (
                &["a+b?c{1}|d~e/f*g.h"],
                &["a+b?c{1}|d~e/f*g.h"],
                &["a+b?c{1}|d~e/f*gxh"],
            ),
            (&["A+", "A-", "Aー"], &["A+", "A-", "Aー"], &["A0", "A\\"]),
            (&["a-", "a^"], &["a-", "a^"], &["ab"]),
            (&["a]", "a\\", "a^", "a-"], &["a]", "a\\", "a^", "a-"], &["ab"]),
            (
                &["a(", "a)", "a+", "a?", "a{"],
                &["a(", "a)", "a+", "a?", "a{"],
                &["a\\"],
            ),
        ];

        let mut items = Vec::new();
        for op in [RegexOperator::Vim, RegexOperator::VimNonNewline] {
            for (words, should_match, should_not_match) in cases {
                let (_, re) = generate_with_both_generators(words, &op);
                let anchored = format!("^{}$", re);
                for s in *should_match {
                    items.push(format!("[{}, {}, 1]", vim_str(&anchored), vim_str(s)));
                }
                for s in *should_not_match {
                    items.push(format!("[{}, {}, 0]", vim_str(&anchored), vim_str(s)));
                }
            }
        }
        let cmd = format!(
            "let cases = [{}] | for c in cases | if (c[1] =~# c[0]) != c[2] | verbose echo 'FAIL' c | cquit! | endif | endfor | qall!",
            items.join(", ")
        );

        match Command::new("vim")
            .args(["-Nu", "NONE", "-es", "-c", &cmd])
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(out) => {
                assert!(
                    out.status.success(),
                    "vim regexp verification failed: {} {}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Skip if Vim is not installed in the test environment.
            }
            Err(e) => panic!("failed to invoke vim: {}", e),
        }
    }
}
