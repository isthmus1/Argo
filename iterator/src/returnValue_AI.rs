use std::process::Command;

/// Compiles the given C source, runs it, and returns whatever it printed to stdout.
pub fn compiled_c_return_val(code: &str) -> String {
    let c_file = "temp.c";
    let exe_file = "temp";

    if let Err(err) = std::fs::write(c_file, code) {
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

            // Clean up both artifacts regardless of how the program ran.
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