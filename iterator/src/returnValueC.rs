use std::process::Command;

/// Compiles a C snippet by wrapping it in a minimal `int main(void)` program,
/// runs it, and returns anything it printed to stdout.
pub fn compiled_c_return_val(code: &str) -> String {
    let c_file = "temp.c";
    let exe_file = "temp";

    let libraries = get_libraries();
    let variables = get_variables();
    let print = get_print();

    // Overhead added around the caller's code.
    let source = format!(
        "{libraries}\n{variables}\n#include <stdio.h>\n\nint main(void) {{\n{code}\n{print}\n return 0;\n}}\n"
    );

    if let Err(err) = std::fs::write(c_file, source) {
        return format!("error: failed to write {c_file}: {err}");
    }

    let compiler = std::env::var("CC").unwrap_or_else(|_| String::from("cc"));

    match Command::new(&compiler)
        .arg(c_file)
        .arg("-o")
        .arg(exe_file)
        .output()
    {
        Err(err) => {
            let _ = std::fs::remove_file(c_file);
            format!("error: failed to run compiler `{compiler}`: {err}")
        }
        Ok(compiled) => {
            if !compiled.status.success() {
                let stderr = String::from_utf8_lossy(&compiled.stderr);
                let _ = std::fs::remove_file(c_file);
                let _ = std::fs::remove_file(exe_file);
                return format!("compile error:\n{}", stderr.trim_end());
            }

            let run_result = Command::new(format!("./{exe_file}")).output();

            let _ = std::fs::remove_file(c_file);
            let _ = std::fs::remove_file(exe_file);

            match run_result {
                Err(err) => format!("error: failed to run {exe_file}: {err}"),
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if output.status.success() {
                        stdout.to_string()
                    } else {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        format!(
                            "runtime error (exit {:?}):\n{}{}",
                            output.status.code(),
                            stdout,
                            stderr
                        )
                    }
                }
            }
        }
    }
}

fn get_libraries() -> String {
    return String::from("");
}

fn get_variables() -> String {
    return String::from("");
}

fn get_print() -> String {
    return String::from("");
}

