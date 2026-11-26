//! `rust2bmb [-o DIR] FILE.rs[=UNIT]..`: converts each file into a BMB unit
//! (named UNIT, or after the file). The units go to DIR/UNIT.bmb, or one
//! after another to standard output. Standard error gets one report line per
//! construct that was not rewritten (`file:line:col: kind: reason`) and, per
//! file, a summary with the counts.

use std::process::ExitCode;

const USAGE: &str = "usage: rust2bmb [-o DIR] FILE.rs[=UNIT]..";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut out_dir: Option<String> = None;
    let mut inputs: Vec<(String, String)> = Vec::new();
    while let Some(a) = args.next() {
        if a == "-o" {
            out_dir = args.next();
        } else if a.starts_with('-') {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        } else {
            let (path, unit) = match a.split_once('=') {
                Some((p, u)) => (p.to_string(), u.to_string()),
                None => {
                    let stem = std::path::Path::new(&a)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (a.clone(), stem)
                }
            };
            inputs.push((path, unit));
        }
    }
    if inputs.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let mut sources = Vec::new();
    for (path, _) in &inputs {
        match std::fs::read_to_string(path) {
            Ok(s) => sources.push(s),
            Err(e) => {
                eprintln!("rust2bmb: {path}: {e}");
                return ExitCode::from(2);
            }
        }
    }
    let pairs: Vec<(&str, &str)> = sources
        .iter()
        .zip(&inputs)
        .map(|(s, (_, u))| (s.as_str(), u.as_str()))
        .collect();
    let outputs = match rust2bmb::convert_all(&pairs) {
        Ok(o) => o,
        Err((k, e)) => {
            let s = e.span().start();
            eprintln!(
                "{}:{}:{}: parse error: {e}",
                inputs[k].0,
                s.line,
                s.column + 1
            );
            return ExitCode::from(1);
        }
    };
    for (((path, unit), src), result) in inputs.iter().zip(&sources).zip(&outputs) {
        match &out_dir {
            Some(dir) => {
                let file = std::path::Path::new(dir).join(format!("{unit}.bmb"));
                if let Err(e) = std::fs::write(&file, &result.text) {
                    eprintln!("rust2bmb: {}: {e}", file.display());
                    return ExitCode::from(2);
                }
            }
            None => print!("{}", result.text),
        }
        for f in &result.findings {
            eprintln!("{path}:{}:{}: {}: {}", f.line, f.column, f.kind, f.reason);
        }
        eprintln!(
            "summary {path}: {} code lines, {} items: {} written ({} with no report), {} dropped; {} reports, {} rewrites",
            rust2bmb::code_lines(src),
            result.items,
            result.items_emitted,
            result.items_clean,
            result.items - result.items_emitted,
            result.findings.len(),
            result.rewrites.values().sum::<usize>()
        );
        for (k, n) in result.kinds() {
            eprintln!("report {path}: {n} {k}");
        }
        for (k, n) in &result.rewrites {
            eprintln!("rewrite {path}: {n} {k}");
        }
    }
    ExitCode::SUCCESS
}
