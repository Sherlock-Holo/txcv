pub mod text_translate {
    use serde::{Deserialize, Serialize};
    use tencentcloud::api::Api;

    #[derive(Debug, Copy, Clone)]
    pub struct TextTranslate;

    #[derive(Debug, Clone, Serialize)]
    pub struct TextTranslateRequest {
        #[serde(rename = "SourceText")]
        pub source_text: String,
        #[serde(rename = "Source")]
        pub source: String,
        #[serde(rename = "Target")]
        pub target: String,
        #[serde(rename = "ProjectId")]
        pub project_id: i64,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct TextTranslateResponse {
        #[serde(rename = "Source")]
        pub source: String,
        #[serde(rename = "Target")]
        pub target: String,
        #[serde(rename = "TargetText")]
        pub target_text: String,
    }

    impl Api for TextTranslate {
        type Request = TextTranslateRequest;
        type Response = TextTranslateResponse;
        const VERSION: &'static str = "2018-03-21";
        const ACTION: &'static str = "TextTranslate";
        const SERVICE: &'static str = "tmt";
        const HOST: &'static str = "tmt.tencentcloudapi.com";
    }
}

pub mod language_detect {
    use serde::{Deserialize, Serialize};
    use tencentcloud::api::Api;

    #[derive(Debug, Copy, Clone)]
    pub struct LanguageDetect;

    #[derive(Debug, Clone, Serialize)]
    pub struct LanguageDetectRequest {
        #[serde(rename = "Text")]
        pub text: String,
        #[serde(rename = "ProjectId")]
        pub project_id: i64,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct LanguageDetectResponse {
        #[serde(rename = "Lang")]
        pub lang: String,
    }

    impl Api for LanguageDetect {
        type Request = LanguageDetectRequest;
        type Response = LanguageDetectResponse;
        const VERSION: &'static str = "2018-03-21";
        const ACTION: &'static str = "LanguageDetect";
        const SERVICE: &'static str = "tmt";
        const HOST: &'static str = "tmt.tencentcloudapi.com";
    }
}
