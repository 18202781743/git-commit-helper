use crate::translator::{CommitMessage, ai_service};
use crate::review;
use dialoguer::Confirm;
use log::{debug, info};
use std::path::Path;
use textwrap::fill;

const MAX_LINE_LENGTH: usize = 72;

fn is_auto_generated_commit(title: &str) -> bool {
    let patterns = ["Merge", "Cherry-pick", "Revert"];
    patterns.iter().any(|pattern| title.starts_with(pattern))
}

pub async fn process_commit_msg(path: &Path, no_review: bool) -> anyhow::Result<()> {
    debug!("开始处理提交消息: {}", path.display());
    let content = std::fs::read_to_string(path)?;
    let msg = CommitMessage::parse(&content);

    // 首先执行代码审查
    let config = crate::config::Config::load()?;
    let review_result = if review::should_skip_review(&msg.title) {
        info!("检测到自动生成的提交消息，跳过代码审查");
        None
    } else {
        info!("正在进行代码审查...");
        let result = review::review_changes(&config, no_review).await?;
        if let Some(review) = &result {
            // 直接在终端显示审查结果
            println!("\n{}\n", review);
        }
        result
    };

    // 检查是否是自动生成的提交消息
    if is_auto_generated_commit(&msg.title) {
        debug!("检测到自动生成的提交消息，跳过翻译");
        return Ok(());
    }

    // 检查是否需要翻译
    if !contains_chinese(&msg.title) {
        debug!("未检测到中文内容，跳过翻译");
        return Ok(());
    }

    info!("检测到中文内容，准备翻译");

    if !Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
        .with_prompt("检测到提交信息包含中文，是否需要翻译？")
        .default(true)
        .interact()? {
        return Ok(());
    }

    info!("开始翻译流程，默认使用 {:?} 服务", config.default_service);

    // 翻译标题
    let en_title = ai_service::translate_with_fallback(&config, &msg.title).await?;
    let en_title = wrap_text(&en_title, MAX_LINE_LENGTH);
    let original_title = msg.title.clone();

    // 翻译正文（如果有的话）
    let (en_body, cn_body) = if let Some(body) = &msg.body {
        let en_body = ai_service::translate_with_fallback(&config, body).await?;
        (Some(wrap_text(&en_body, MAX_LINE_LENGTH)), Some(body.clone()))
    } else {
        (None, None)
    };

    // 构建新的消息结构
    let mut body_parts = Vec::new();

    // 添加代码审查报告（如果有）
    if let Some(review) = review_result {
        body_parts.push(review);
        body_parts.push(String::new());  // 空行分隔
    }

    // 添加英文和中文内容
    if let Some(en_body) = en_body {
        body_parts.push(en_body);
        body_parts.push(String::new());
    }

    body_parts.push(original_title);

    if let Some(body) = cn_body {
        body_parts.push(String::new());
        body_parts.push(body);
    }

    let new_msg = CommitMessage {
        title: en_title,
        body: Some(body_parts.join("\n")),
        marks: vec![],
    };

    info!("翻译完成，正在写入文件");
    std::fs::write(path, new_msg.format())?;
    info!("处理完成");
    Ok(())
}

pub fn format_body(en_body: Option<&str>, cn_title: &str, cn_body: Option<&str>, marks: &[String]) -> String {
    let mut parts = Vec::new();

    // 1. 英文正文
    if let Some(body) = en_body {
        parts.push(body.to_string());
        parts.push(String::new());  // 空行分隔
    }

    // 2. 中文标题
    parts.push(cn_title.to_string());

    // 3. 中文正文
    if let Some(body) = cn_body {
        parts.push(String::new());  // 空行分隔
        parts.push(body.to_string());
    }

    // 4. 其他标记
    if !marks.is_empty() {
        parts.push(String::new());  // 空行分隔
        parts.extend(marks.iter().cloned());
    }

    parts.join("\n")
}

fn contains_chinese(text: &str) -> bool {
    text.chars().any(|c| c as u32 >= 0x4E00 && c as u32 <= 0x9FFF)
}

pub fn wrap_text(text: &str, max_length: usize) -> String {
    fill(text, max_length)
}
