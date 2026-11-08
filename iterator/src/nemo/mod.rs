use serde_json::Value;
const CONTEXT_LINES: u32 = 10;
//const testCode: &str = include_str!("../examples/internet_example.c");

pub fn getLines(code: &str, startLine: u32, endLine: u32) -> String {
    if startLine == 0 || endLine < startLine {
        return String::new();
    }

    let start = startLine as usize;
    let end = endLine as usize;

    code.lines()
        .skip(start - 1)
        .take(end - start + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

//For reasoning content
fn text_at(value: &Value, path: &str) -> Option<String> {
    match value.pointer(path) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::Array(parts)) => {
            let text = parts
                .iter()
                .filter_map(|part| part.pointer("/text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}

/// One chat completion with an explicit `model` (so a caller can escalate), against the
/// OpenAI-compatible endpoint in `OPENAI_BASE_URL`, authorized by `OPENAI_API_KEY`. Returns the
/// reply text; the caller owns the prompt.
pub async fn complete(
    model: &str,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|e| format!("OPENAI_API_KEY: {e}"))?;
    let base_url = std::env::var("OPENAI_BASE_URL").map_err(|e| format!("OPENAI_BASE_URL: {e}"))?;
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }]
    });
    let response: Value = reqwest::Client::new()
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    text_at(&response, "/choices/0/message/content")
        .ok_or_else(|| format!("no text content in response: {response}").into())
}

#[tokio::main]
async fn sendPrompt(
    source: &str,
    start_line: u32,
    end_line: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    if start_line == 0 || end_line < start_line {
        return Err(
            format!("invalid line range: start_line={start_line}, end_line={end_line}").into(),
        );
    }

    let context_start = start_line.saturating_sub(CONTEXT_LINES).max(1);
    let context_end = end_line.saturating_add(CONTEXT_LINES);
    let before = getLines(source, context_start, start_line.saturating_sub(1));
    let after = getLines(source, end_line.saturating_add(1), context_end);
    let target = getLines(source, start_line, end_line);

    let mut context = String::new();
    if !before.is_empty() {
        context.push_str(&format!(
            "BEFORE (lines {context_start}-{}, for reference only, do NOT output):\n{before}\n",
            start_line - 1
        ));
    }
    if !after.is_empty() {
        context.push_str(&format!(
            "AFTER (lines {}-{context_end}, for reference only, do NOT output):\n{after}\n",
            end_line.saturating_add(1)
        ));
    }

    let specPrompt = format!(
        "You're not allowed to say anything that isn't code. This is either comments or c code. Take the TARGET lines and convert them into rust. The BEFORE and AFTER lines are surrounding reference only: do NOT rewrite, repeat, or output them. Only output the rewritten TARGET lines. You're ONLY allowed to give back code, if you need to say something, it has to be consize and in the form of rust comments. never turn rust code into comments and never give back comments without comment marks. If you have ANYTHING to say that isn't code it has to be in rust comment form. Preserve the exact C semantics, not just the values visible in this snippet: C treats integers as truthy when nonzero, so translate `while (x)` as `while x != 0` and `if (p)` as `if p != 0`, keeping the correct signedness and zero/negative behavior. Every C local that is assigned or reassigned after its declaration must be declared with `let mut` in Rust. The Rust you output must be valid and compilable. When the TARGET is a whole program, output a self-contained Rust program that compiles on its own and preserves the C program's observable behavior, including stdout content and order and exit behavior.\n{context}TARGET (lines {start_line}-{end_line}, rewrite ONLY these into rust):\n{target}\n"
    );

    let api_key = std::env::var("OPENAI_API_KEY")?;
    let base_url =
        std::env::var("OPENAI_BASE_URL").map(|value| value.trim_end_matches('/').to_owned())?;
    let model = std::env::var("OPENAI_MODEL")?;
    let finalPrompt = specPrompt;

    let url = format!("{base_url}/v1/chat/completions");
    let body = serde_json::json!({
        "model": model,
        //"max_tokens": 512,
        "messages": [
            {
                "role": "user",
                "content": finalPrompt
            }
        ]
    });

    println!("{}", finalPrompt);

    let client = reqwest::Client::new();
    let response: Value = client
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let text = text_at(&response, "/choices/0/message/content");
    //For reasoning content
    //.or_else(|| text_at(&response, "/choices/0/message/reasoning_content"));

    match text {
        Some(content) => println!("CODING AGENT CONTENT:\n{content}"),
        None => eprintln!("no text content in response: {response}"),
    }

    Ok(())
}
