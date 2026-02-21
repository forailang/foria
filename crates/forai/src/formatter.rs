use std::fs;
use std::path::{Path, PathBuf};

const INDENT: &str = "    ";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Block {
    Decl,     // func, flow, sink, source header
    Body,     // body, init, from content
    Docs,     // docs block
    Test,     // test block
    TypeDecl, // type, data, enum
    Case,     // case block
    Arm,      // when/else arm inside case
    If,       // if/else-if/else arm
    Loop,     // loop block
    Sync,     // sync block
    Step,     // step...then block
}

/// Format a `.fa` source string, returning the formatted version.
pub fn format_source(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut stack: Vec<Block> = Vec::new();
    let mut prev_blank = false;
    let mut prev_was_uses = false;

    for raw_line in &lines {
        let trimmed = raw_line.trim();

        if trimmed.is_empty() {
            if !prev_blank && !out.is_empty() {
                out.push(String::new());
                prev_blank = true;
            }
            prev_was_uses = false;
            continue;
        }

        let first_word = first_token(trimmed);
        let is_else_if = trimmed.starts_with("else if ");

        // --- Pops (dedent before printing) ---
        match first_word {
            "done" => {
                // Pop arm if present, then pop the structural block
                if stack.last() == Some(&Block::Arm) {
                    stack.pop();
                }
                stack.pop(); // structural block (Case, If, Loop, Body, Docs, etc.)
            }
            "body" | "init" => {
                // Close the Decl header
                if stack.last() == Some(&Block::Decl) {
                    stack.pop();
                }
            }
            "from" => {
                // Close Body (from init) or Decl (source without init)
                match stack.last() {
                    Some(&Block::Body) | Some(&Block::Decl) => { stack.pop(); }
                    _ => {}
                }
            }
            "when" => {
                // Close previous arm if present
                if stack.last() == Some(&Block::Arm) {
                    stack.pop();
                }
            }
            "else" if is_else_if => {
                // Close previous If arm
                if stack.last() == Some(&Block::If) {
                    stack.pop();
                }
            }
            "else" => {
                // Could be case-else (pop Arm) or if-else (pop If)
                match stack.last() {
                    Some(&Block::Arm) => { stack.pop(); }
                    Some(&Block::If) => { stack.pop(); }
                    _ => {}
                }
            }
            _ => {}
        }

        let indent = stack.len();

        // --- Blank line insertion between top-level declarations ---
        if indent == 0 {
            let is_uses = first_word == "uses";
            if !out.is_empty() {
                let need_blank = if is_uses && prev_was_uses {
                    false
                } else {
                    is_top_level_keyword(first_word)
                };
                if need_blank && !prev_blank {
                    out.push(String::new());
                }
            }
            prev_was_uses = is_uses;
        } else {
            prev_was_uses = false;
        }

        // --- Format and output the line ---
        let formatted_content = format_line_content(trimmed);
        let indented = if indent > 0 {
            format!("{}{}", INDENT.repeat(indent), formatted_content)
        } else {
            formatted_content
        };
        out.push(indented);
        prev_blank = false;

        // --- Pushes (indent after printing) ---
        let single_line_done = trimmed.ends_with("done");
        match first_word {
            "func" | "flow" | "sink" | "source" => {
                stack.push(Block::Decl);
            }
            "body" | "init" => {
                stack.push(Block::Body);
            }
            "from" if !single_line_done => {
                stack.push(Block::Body);
            }
            "docs" => {
                stack.push(Block::Docs);
            }
            "test" => {
                stack.push(Block::Test);
            }
            "type" | "data" | "enum" if !single_line_done => {
                stack.push(Block::TypeDecl);
            }
            "case" => {
                stack.push(Block::Case);
            }
            "when" => {
                stack.push(Block::Arm);
            }
            "else" if is_else_if => {
                if !single_line_done {
                    stack.push(Block::If);
                }
            }
            "else" => {
                // Push same type as what was popped
                if stack.last() == Some(&Block::Case) {
                    stack.push(Block::Arm); // case else
                } else {
                    stack.push(Block::If); // if else
                }
            }
            "if" if !single_line_done => {
                stack.push(Block::If);
            }
            "loop" if !single_line_done => {
                stack.push(Block::Loop);
            }
            "sync" if !single_line_done => {
                stack.push(Block::Sync);
            }
            "step" if trimmed.contains(" then") && !single_line_done => {
                stack.push(Block::Step);
            }
            _ => {
                // Check for sync assignment: `[vars] = sync` or `x = sync`
                if !single_line_done && line_is_sync_assignment(trimmed) {
                    stack.push(Block::Sync);
                }
            }
        }
    }

    // Remove trailing blank lines
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }

    let mut result = out.join("\n");
    if !result.is_empty() {
        result.push('\n');
    }
    result
}

/// Returns true if the source is already formatted.
pub fn check_formatted(source: &str) -> bool {
    format_source(source) == source
}

/// Format all `.fa` files under a path (file or directory).
/// Returns (formatted_files, total_files).
pub fn fmt_path(path: &Path, check_only: bool) -> Result<(Vec<PathBuf>, usize), String> {
    let files = collect_fa_files(path)?;
    let total = files.len();
    let mut changed = Vec::new();

    for file in &files {
        let source = fs::read_to_string(file)
            .map_err(|e| format!("failed to read {}: {e}", file.display()))?;
        let formatted = format_source(&source);
        if formatted != source {
            changed.push(file.clone());
            if !check_only {
                fs::write(file, &formatted)
                    .map_err(|e| format!("failed to write {}: {e}", file.display()))?;
            }
        }
    }

    Ok((changed, total))
}

fn collect_fa_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("fa") {
            return Ok(vec![path.to_path_buf()]);
        }
        return Err(format!("{} is not a .fa file", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("{} does not exist", path.display()));
    }
    let mut files = Vec::new();
    collect_fa_recursive(path, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_fa_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("directory entry error: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "dist" || name == "node_modules" || name == "docs" {
                continue;
            }
            collect_fa_recursive(&path, files)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("fa") {
            files.push(path);
        }
    }
    Ok(())
}

fn first_token(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("")
}

fn is_top_level_keyword(word: &str) -> bool {
    matches!(
        word,
        "func" | "flow" | "sink" | "source" | "docs" | "test"
            | "type" | "data" | "enum" | "uses"
    )
}

fn line_is_sync_assignment(trimmed: &str) -> bool {
    // Matches `x = sync` or `[a, b] = sync`
    if let Some(pos) = trimmed.find("= sync") {
        let after = &trimmed[pos + 6..];
        after.is_empty() || after.starts_with(' ') || after.starts_with('\n')
    } else {
        false
    }
}

fn format_line_content(trimmed: &str) -> String {
    if trimmed.starts_with('#') {
        if trimmed.len() > 1 && !trimmed.starts_with("# ") && !trimmed.starts_with("#!") {
            return format!("# {}", &trimmed[1..].trim_start());
        }
        return trimmed.to_string();
    }
    normalize_spaces(trimmed)
}

/// Collapse runs of spaces outside string literals.
fn normalize_spaces(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut in_string = false;
    let mut escape = false;

    for c in line.chars() {
        if escape {
            result.push(c);
            escape = false;
            continue;
        }
        if c == '\\' && in_string {
            result.push(c);
            escape = true;
            continue;
        }
        if c == '"' {
            in_string = !in_string;
            result.push(c);
            continue;
        }
        if in_string {
            result.push(c);
            continue;
        }
        if c == ' ' {
            if !result.ends_with(' ') {
                result.push(' ');
            }
            continue;
        }
        result.push(c);
    }

    while result.ends_with(' ') {
        result.pop();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_func() {
        let input = "\
docs Hello
    Says hello.
done

func Hello
    take name as text
    emit result as text
    fail error as text
body
    greeting = \"Hello #{name}!\"
    emit greeting
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input, "already-formatted input should be unchanged");
    }

    #[test]
    fn fixes_indentation() {
        let input = "func Hello\ntake name as text\nemit result as text\nfail error as text\nbody\ngreeting = \"hi\"\nemit greeting\ndone\n";
        let expected = "func Hello\n    take name as text\n    emit result as text\n    fail error as text\nbody\n    greeting = \"hi\"\n    emit greeting\ndone\n";
        assert_eq!(format_source(input), expected);
    }

    #[test]
    fn case_when_indent() {
        let input = "\
func Test
    take x as text
    emit result as text
    fail error as text
body
    case x
        when \"a\"
            r = \"alpha\"
            emit r
        when \"b\"
            r = \"beta\"
            emit r
        else
            r = \"other\"
            emit r
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn uses_grouping() {
        let input = "uses alpha\nuses beta\n\ndocs Foo\n  A thing.\ndone\n\nfunc Foo\n  take x as text\n  emit result as text\n  fail error as text\nbody\n  emit x\ndone\n";
        let formatted = format_source(input);
        assert!(formatted.contains("uses alpha\nuses beta\n"), "uses should be grouped");
        assert!(formatted.contains("uses beta\n\ndocs Foo"), "blank line before docs");
    }

    #[test]
    fn collapses_multiple_blanks() {
        let input = "uses a\n\n\n\ndocs Foo\n    Hi.\ndone\n";
        let formatted = format_source(input);
        assert!(formatted.contains("uses a\n\ndocs Foo"));
        assert!(!formatted.contains("\n\n\n"));
    }

    #[test]
    fn comment_space() {
        let input = "#comment\n# already spaced\n";
        let formatted = format_source(input);
        assert!(formatted.contains("# comment\n"));
        assert!(formatted.contains("# already spaced\n"));
    }

    #[test]
    fn trailing_whitespace_removed() {
        let input = "func Hello   \n    take x as text   \n    emit result as text\n    fail error as text\nbody\n    emit x\ndone\n";
        let formatted = format_source(input);
        for line in formatted.lines() {
            assert_eq!(line, line.trim_end(), "no trailing whitespace: {:?}", line);
        }
    }

    #[test]
    fn final_newline() {
        let input = "uses foo";
        let formatted = format_source(input);
        assert!(formatted.ends_with('\n'));
    }

    #[test]
    fn if_else_indent() {
        let input = "\
func Test
    take x as bool
    emit result as text
    fail error as text
body
    if x
        r = \"yes\"
        emit r
    else
        r = \"no\"
        emit r
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn nested_if_else_if() {
        let input = "\
func Classify
    take cmd as text
    emit result as text
    fail error as text
body
    if cmd == \"help\"
        r = \"help\"
        emit r
    else if cmd == \"ls\"
        r = \"ls\"
        emit r
    else
        r = \"unknown\"
        emit r
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn loop_indent() {
        let input = "\
func Test
    take items as list
    emit result as text
    fail error as text
body
    iters = list.range(0, 10)
    loop iters as i
        _ = term.print(i)
    done
    ok = \"done\"
    emit ok
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn flow_step_then() {
        let input = "\
flow Start
    emit result as text
    fail error as text
body
    step display.Welcome() done
    step sources.Commands() then
        next :cmd to cmd
    done
    step router.Classify(cmd to :cmd) then
        next :result to kind
        case kind
            when \"help\"
                step data.HelpText() then
                    next :result to output
                done
                step display.Print(output to :text) done
            else
                step display.PrintError(cmd to :cmd) done
        done
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn source_decl() {
        let input = "\
source Commands
    emit cmd as text
    fail error as text
from term.prompt(\"docs> \") as raw
    trimmed = str.trim(raw)
    emit trimmed
    case trimmed
        when \"quit\"
            break
        when \"exit\"
            break
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn source_with_init() {
        let input = "\
source HTTPRequests
    take port as long
    emit req as dict
    fail error as text
init
    srv = http.server.listen(port)
from http.server.accept(srv)
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn check_formatted_returns_true() {
        let input = "uses foo\n\ndocs Bar\n    Hi.\ndone\n";
        let formatted = format_source(input);
        assert!(check_formatted(&formatted));
    }

    #[test]
    fn idempotent() {
        let input = "\
uses app

docs main
    Interactive docs browser.
done

flow main
    emit result as text
    fail error as text
body
    step app.Start() done
done
";
        let once = format_source(input);
        let twice = format_source(&once);
        assert_eq!(once, twice, "formatting should be idempotent");
    }

    #[test]
    fn docs_with_nested_docs() {
        let input = "\
docs StartResult
    Result of the session.

    docs status
        Exit status message.
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn type_decl() {
        let input = "\
type StartResult
    status text
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn test_block() {
        let input = "\
test Classify
    must Classify(\"help\") == \"help\"
    must Classify(\"quit\") == \"quit\"
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn state_and_send_nowait() {
        let input = "\
flow Start
    emit result as text
    fail error as text
body
    state conn = db.open(\"factory.db\")
    step db.Migrate(conn to :conn) done
    send nowait workflow.RunJobLoop()
    step sources.HTTPRequests(8080 to :port) then
        next :req to req
    done
    step handler.HandleRequest(conn to :conn, req to :req) done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn else_if_is_dedent_then_indent() {
        let input = "\
func T
    take x as long
    emit result as text
    fail error as text
body
    if x == 1
        r = \"one\"
        emit r
    else if x == 2
        r = \"two\"
        emit r
    else
        r = \"other\"
        emit r
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn case_inside_if() {
        let input = "\
func T
    take x as bool
    take y as text
    emit result as text
    fail error as text
body
    if x
        case y
            when \"a\"
                r = \"alpha\"
                emit r
            else
                r = \"other\"
                emit r
        done
    else
        r = \"no\"
        emit r
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn if_inside_case() {
        let input = "\
func T
    take x as text
    take y as bool
    emit result as text
    fail error as text
body
    case x
        when \"a\"
            if y
                r = \"yes-a\"
                emit r
            else
                r = \"no-a\"
                emit r
            done
        else
            r = \"other\"
            emit r
    done
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, input);
    }

    #[test]
    fn real_example_classify() {
        // From examples/read-docs/app/router/Classify.fa (will be reformatted to 4-space)
        let input = "\
docs Classify
  Categorizes a user command into a routing label.
  Returns \"help\", \"ls\", \"quit\", \"ns\", \"op\", or \"unknown\".
done

func Classify
  take cmd as text
  emit result as text
  fail error as text
body
  if cmd == \"help\"
    r = \"help\"
    emit r
  else if cmd == \"ls\"
    r = \"ls\"
    emit r
  else
    r = \"unknown\"
    emit r
  done
done

test Classify
  must Classify(\"help\") == \"help\"
  must Classify(\"quit\") == \"quit\"
done
";
        let expected = "\
docs Classify
    Categorizes a user command into a routing label.
    Returns \"help\", \"ls\", \"quit\", \"ns\", \"op\", or \"unknown\".
done

func Classify
    take cmd as text
    emit result as text
    fail error as text
body
    if cmd == \"help\"
        r = \"help\"
        emit r
    else if cmd == \"ls\"
        r = \"ls\"
        emit r
    else
        r = \"unknown\"
        emit r
    done
done

test Classify
    must Classify(\"help\") == \"help\"
    must Classify(\"quit\") == \"quit\"
done
";
        let formatted = format_source(input);
        assert_eq!(formatted, expected);
    }

    #[test]
    fn format_is_idempotent_on_output() {
        let messy = "func Foo\ntake x as text\nemit result as text\nfail error as text\nbody\ncase x\nwhen \"a\"\nr = \"alpha\"\nemit r\nelse\nr = \"other\"\nemit r\ndone\ndone\n";
        let once = format_source(messy);
        let twice = format_source(&once);
        assert_eq!(once, twice, "formatting must be idempotent");
    }
}
