use std::borrow::Cow;

use crate::settings::{PromptCatalogItem, PromptOverrides, UiLanguage};

const TRANSLATE_EN_TO_JA: &str = r#"あなたはプロの翻訳家であり、ネイティブスピーカーです。入力された英語のテキストを自然な日本語に翻訳してください。

出力は翻訳文のみとし、見出し・ラベル（「翻訳」など）・前置き・解説・ニュアンス説明・別の表現案・箇条書きは一切出力しないでください。段落が必要な場合は空行で区切ってよい。"#;

const TRANSLATE_JA_TO_EN: &str = r#"You are a professional translator and native speaker. Translate the user's Japanese input into natural English.

Output only the translation. Do not output headings, labels (such as "Translation"), preambles, explanations, nuance notes, alternative phrasings, or bullet points. Use a blank line between paragraphs if needed."#;

const OPENAI_TRANSCRIPTION: &str = "日本語の音声です。固有名詞と専門用語を正確に文字起こししてください。{{dictionary_context_block}}";

const GEMINI_AUDIO_SYSTEM: &str = "あなたは日本語音声の文字起こし専門家です。";

const GEMINI_AUDIO_USER: &str =
    "添付された日本語音声を、できるだけ正確に文字起こししてください。出力は文字起こし本文のみ。{{dictionary_context_block}}";

const ASK_WITHOUT_SELECTION_SYSTEM: &str = "あなたは日本語の音声入力編集者です。音声指示だけを根拠に、ユーザーがそのまま貼り付けられる最終本文を作ります。出力は最終本文のみ。前置き、説明、引用符、ラベルは出しません。";

const ASK_WITHOUT_SELECTION_USER: &str = r#"{{dictionary_section}}

{{screen_context}}

選択中テキストは取得できませんでした。

音声指示の文字起こし:
{{transcript}}

要件:
- 音声指示だけに基づいて最終本文を作る。
- 画面文脈は入力先、周辺表示、固有名詞、文体のヒントとしてだけ使う。
- 選択されていない文章、過去のクリップボード、過去の会話内容を推測して混ぜない。
- 音声指示または文脈から該当語だと判断できる場合だけ、辞書の優先表記に整える。
- 該当すると判断できない語を辞書語へ置き換えない。
- 内容を勝手に増やさない。"#;

const ASK_WITH_SELECTION_SYSTEM: &str = "あなたは日本語の文章編集者です。選択中テキストを、音声指示に従って書き換えます。出力は置換後の本文のみ。前置き、説明、引用符、ラベルは出しません。";

const ASK_WITH_SELECTION_USER: &str = r#"{{dictionary_section}}

{{screen_context}}

選択中テキスト:
{{selected_text}}

音声指示の文字起こし:
{{transcript}}

要件:
- 音声指示に従って選択中テキストを書き換える。
- 指示が曖昧な場合は、選択中テキストの意味を保ったまま自然に整える。
- 画面文脈は選択中テキストの周辺、相手、文体、固有名詞のヒントとしてだけ使う。
- 音声指示、選択中テキスト、文脈から該当語だと判断できる場合だけ、辞書の優先表記に整える。
- 該当すると判断できない語を辞書語へ置き換えない。
- 出力は置換する本文のみ。"#;

pub fn catalog() -> Vec<PromptCatalogItem> {
    vec![
        catalog_item(
            "translateEnToJa",
            "翻訳: 英語 → 日本語",
            5,
            &[],
            TRANSLATE_EN_TO_JA,
        ),
        catalog_item(
            "translateJaToEn",
            "翻訳: 日本語 → 英語",
            5,
            &[],
            TRANSLATE_JA_TO_EN,
        ),
        catalog_item(
            "openaiTranscription",
            "OpenAI文字起こし",
            3,
            &[],
            OPENAI_TRANSCRIPTION,
        ),
        catalog_item(
            "geminiAudioSystem",
            "Gemini音声: system",
            2,
            &[],
            GEMINI_AUDIO_SYSTEM,
        ),
        catalog_item(
            "geminiAudioUser",
            "Gemini音声: user",
            3,
            &[],
            GEMINI_AUDIO_USER,
        ),
        catalog_item(
            "askWithoutSelectionSystem",
            "Ask（選択なし）: system",
            3,
            &[],
            ASK_WITHOUT_SELECTION_SYSTEM,
        ),
        catalog_item(
            "askWithoutSelectionUser",
            "Ask（選択なし）: user",
            8,
            &["{{transcript}}"],
            ASK_WITHOUT_SELECTION_USER,
        ),
        catalog_item(
            "askWithSelectionSystem",
            "Ask（選択あり）: system",
            3,
            &[],
            ASK_WITH_SELECTION_SYSTEM,
        ),
        catalog_item(
            "askWithSelectionUser",
            "Ask（選択あり）: user",
            8,
            &["{{selected_text}}", "{{transcript}}"],
            ASK_WITH_SELECTION_USER,
        ),
    ]
}

fn catalog_item(
    key: &str,
    label: &str,
    rows: u8,
    required: &[&str],
    default_text: &str,
) -> PromptCatalogItem {
    PromptCatalogItem {
        key: key.to_string(),
        label: label.to_string(),
        rows,
        required: required.iter().map(|token| token.to_string()).collect(),
        default_text: default_text.to_string(),
    }
}

pub fn validate_overrides(overrides: &PromptOverrides) -> Result<(), String> {
    validate_required(
        "選択なしAskのユーザープロンプト",
        overrides.ask_without_selection_user.as_deref(),
        &["{{transcript}}"],
    )?;
    validate_required(
        "選択ありAskのユーザープロンプト",
        overrides.ask_with_selection_user.as_deref(),
        &["{{selected_text}}", "{{transcript}}"],
    )?;
    Ok(())
}

fn validate_required(label: &str, template: Option<&str>, required: &[&str]) -> Result<(), String> {
    let Some(template) = template else {
        return Ok(());
    };
    for token in required {
        if !template.contains(token) {
            return Err(format!("{label}には {token} を含めてください。"));
        }
    }
    Ok(())
}

pub fn translation_system_prompt(
    overrides: &PromptOverrides,
    source: UiLanguage,
    target: UiLanguage,
) -> Cow<'_, str> {
    match (source, target) {
        (UiLanguage::En, UiLanguage::Ja) => {
            template_or_default(overrides.translate_en_to_ja.as_deref(), TRANSLATE_EN_TO_JA)
        }
        (UiLanguage::Ja, UiLanguage::En) => {
            template_or_default(overrides.translate_ja_to_en.as_deref(), TRANSLATE_JA_TO_EN)
        }
        _ => template_or_default(overrides.translate_en_to_ja.as_deref(), TRANSLATE_EN_TO_JA),
    }
}

/// 文字起こしモデルへ渡す辞書ブロック。辞書語はヒントとして渡し、聞こえていない
/// 語への過剰な置換を抑える。辞書が空なら空文字を返す。
fn dictionary_context_block(dictionary_context: &str) -> String {
    if dictionary_context.trim().is_empty() {
        String::new()
    } else {
        format!(
            concat!(
                "\n\n次の固有名詞・専門用語は辞書ヒントです。",
                "音声が該当語として聞こえる場合だけ、以下の優先表記を使ってください。",
                "該当すると判断できない語を辞書語へ置き換えたり、",
                "聞こえていない辞書語を追加したりしないでください。",
                "該当語はカタカナへの音写や日本語訳をせず、",
                "優先表記のまま出力してください:\n{}"
            ),
            dictionary_context
        )
    }
}

pub fn openai_transcription_prompt(
    overrides: &PromptOverrides,
    dictionary_context: &str,
) -> String {
    render(
        &template_or_default(
            overrides.openai_transcription.as_deref(),
            OPENAI_TRANSCRIPTION,
        ),
        &[(
            "{{dictionary_context_block}}",
            &dictionary_context_block(dictionary_context),
        )],
    )
}

pub fn gemini_audio_system(overrides: &PromptOverrides) -> Cow<'_, str> {
    template_or_default(
        overrides.gemini_audio_system.as_deref(),
        GEMINI_AUDIO_SYSTEM,
    )
}

pub fn gemini_audio_user(overrides: &PromptOverrides, dictionary_context: &str) -> String {
    render(
        &template_or_default(overrides.gemini_audio_user.as_deref(), GEMINI_AUDIO_USER),
        &[(
            "{{dictionary_context_block}}",
            &dictionary_context_block(dictionary_context),
        )],
    )
}

pub fn voice_mode_user_with_context(
    template: &str,
    dictionary_section: &str,
    screen_context_section: &str,
    transcript: &str,
) -> String {
    render_voice_prompt(
        template,
        &[
            ("{{dictionary_section}}", dictionary_section),
            ("{{screen_context}}", screen_context_section),
            ("{{transcript}}", transcript),
        ],
        screen_context_section,
    )
}

pub fn ask_without_selection_system(overrides: &PromptOverrides) -> Cow<'_, str> {
    template_or_default(
        overrides.ask_without_selection_system.as_deref(),
        ASK_WITHOUT_SELECTION_SYSTEM,
    )
}

pub fn ask_without_selection_user(
    overrides: &PromptOverrides,
    dictionary_section: &str,
    screen_context_section: &str,
    transcript: &str,
) -> String {
    let template = template_or_default(
        overrides.ask_without_selection_user.as_deref(),
        ASK_WITHOUT_SELECTION_USER,
    );
    render_voice_prompt(
        &template,
        &[
            ("{{dictionary_section}}", dictionary_section),
            ("{{screen_context}}", screen_context_section),
            ("{{transcript}}", transcript),
        ],
        screen_context_section,
    )
}

pub fn ask_with_selection_system(overrides: &PromptOverrides) -> Cow<'_, str> {
    template_or_default(
        overrides.ask_with_selection_system.as_deref(),
        ASK_WITH_SELECTION_SYSTEM,
    )
}

pub fn ask_with_selection_user(
    overrides: &PromptOverrides,
    dictionary_section: &str,
    screen_context_section: &str,
    selected_text: &str,
    transcript: &str,
) -> String {
    let template = template_or_default(
        overrides.ask_with_selection_user.as_deref(),
        ASK_WITH_SELECTION_USER,
    );
    render_voice_prompt(
        &template,
        &[
            ("{{dictionary_section}}", dictionary_section),
            ("{{screen_context}}", screen_context_section),
            ("{{selected_text}}", selected_text),
            ("{{transcript}}", transcript),
        ],
        screen_context_section,
    )
}

fn template_or_default<'a>(
    override_value: Option<&'a str>,
    default_value: &'static str,
) -> Cow<'a, str> {
    match override_value {
        Some(value) if !value.trim().is_empty() => Cow::Borrowed(value),
        _ => Cow::Borrowed(default_value),
    }
}

fn render(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (token, value) in replacements {
        out = out.replace(token, value);
    }
    out
}

fn render_voice_prompt(
    template: &str,
    replacements: &[(&str, &str)],
    screen_context_section: &str,
) -> String {
    let rendered = render(template, replacements);
    if template.contains("{{screen_context}}") || screen_context_section.trim().is_empty() {
        rendered
    } else {
        format!("{screen_context_section}\n\n{rendered}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_required_placeholder() {
        let overrides = PromptOverrides {
            ask_with_selection_user: Some("{{transcript}}だけ".to_string()),
            ..PromptOverrides::default()
        };
        assert!(validate_overrides(&overrides).is_err());
    }

    #[test]
    fn dictionary_context_block_is_empty_without_terms() {
        assert_eq!(dictionary_context_block("   "), "");
    }

    #[test]
    fn dictionary_context_block_keeps_terms_as_conditional_hints() {
        let block = dictionary_context_block("- Typeless\n- AquaVoice");
        assert!(block.contains("Typeless"));
        assert!(block.contains("AquaVoice"));
        assert!(block.contains("音写"));
        assert!(block.contains("聞こえていない辞書語を追加"));
    }

    #[test]
    fn openai_prompt_embeds_dictionary_terms() {
        let prompt = openai_transcription_prompt(&PromptOverrides::default(), "- Typeless");
        assert!(prompt.contains("Typeless"));
        assert!(prompt.contains("音写"));
        assert!(prompt.contains("該当すると判断できない語"));
    }

    #[test]
    fn voice_prompt_renders_screen_context_token() {
        let prompt = voice_mode_user_with_context(
            "{{dictionary_section}}\n{{screen_context}}\n{{transcript}}",
            "辞書",
            "画面文脈",
            "本文",
        );

        assert!(prompt.contains("辞書"));
        assert!(prompt.contains("画面文脈"));
        assert!(prompt.contains("本文"));
    }

    #[test]
    fn voice_prompt_prepends_screen_context_when_token_is_missing() {
        let prompt = voice_mode_user_with_context("{{transcript}}", "", "画面文脈", "本文");

        assert!(prompt.starts_with("画面文脈\n\n"));
        assert!(prompt.ends_with("本文"));
    }
}
