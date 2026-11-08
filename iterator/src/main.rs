use serde_json::Value;
const testCode: &str = "
 printf(\"Hello, World!\\n\");
";

fn main() {
    if let Err(err) = sendPrompt(getLines(testCode, 3, 1000)) {
        eprintln!("error: {err}");
    }
}

fn getLines(code: &str, startLine: u32, endLine: u32) -> String {
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

#[tokio::main]
async fn sendPrompt(inputCode: String) -> Result<(), Box<dyn std::error::Error>> {
    let specPrompt = "You're not allowed to say anything that isn't code. This is either comments or c code. Take this code and convert it into rust. You're ONLY allowed to give back code, if you need to say something, it has to be consize and in the form of rust comments. never turn rust code into comments and never give back comments without comment marks. If you have ANYTHING to say that isn't code it has to be in rust comment form. This is the code:\n";

    let api_key = std::env::var("OPENAI_API_KEY")?;
    let base_url =
        std::env::var("OPENAI_BASE_URL").map(|value| value.trim_end_matches('/').to_owned())?;
    let model = std::env::var("OPENAI_MODEL")?;
    let finalPrompt = format!("{} {}", specPrompt, inputCode);

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
        Some(content) => println!("{content}"),
        None => eprintln!("no text content in response: {response}"),
    }

    Ok(())
}
