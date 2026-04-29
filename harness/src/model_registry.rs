use serde::{Deserialize, Serialize};

use crate::adapter::HostKind;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InferenceProvider {
    Ollama,
    LmStudio,
}

impl InferenceProvider {
    pub fn from_host_kind(host: HostKind) -> Option<Self> {
        match host {
            HostKind::Ollama => Some(Self::Ollama),
            HostKind::LmStudio => Some(Self::LmStudio),
            _ => None,
        }
    }

    pub fn default_endpoint(self) -> &'static str {
        match self {
            Self::Ollama => "http://127.0.0.1:11434",
            Self::LmStudio => "http://127.0.0.1:1234",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::LmStudio => "lmstudio",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelStrength {
    Coding,
    GeneralReasoning,
    InstructionFollowing,
    LongContext,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupportedModel {
    pub key: &'static str,
    pub family: &'static str,
    pub display_name: &'static str,
    pub params_b: f32,
    pub context_window: u32,
    pub ollama_id: Option<&'static str>,
    pub lmstudio_repo: Option<&'static str>,
    pub recommended_quant: &'static str,
    pub min_vram_gb: u8,
    pub strengths: &'static [ModelStrength],
    pub notes: &'static str,
}

// Curated set. The bar is: GGUF-quantized variants are widely published, the
// model follows structured instructions reliably enough for the harness's
// strict output contracts, and runs on consumer hardware in at least one
// quant tier. Order is small-to-large.
pub const REGISTRY: &[SupportedModel] = &[
    SupportedModel {
        key: "qwen2.5-coder-7b",
        family: "qwen2.5-coder",
        display_name: "Qwen2.5-Coder 7B Instruct",
        params_b: 7.0,
        context_window: 32_768,
        ollama_id: Some("qwen2.5-coder:7b"),
        lmstudio_repo: Some("lmstudio-community/Qwen2.5-Coder-7B-Instruct-GGUF"),
        recommended_quant: "Q4_K_M",
        min_vram_gb: 8,
        strengths: &[ModelStrength::Coding, ModelStrength::InstructionFollowing],
        notes: "Smallest tier that holds up on slice contracts; good fit for shredder/structural personas.",
    },
    SupportedModel {
        key: "qwen2.5-coder-14b",
        family: "qwen2.5-coder",
        display_name: "Qwen2.5-Coder 14B Instruct",
        params_b: 14.0,
        context_window: 32_768,
        ollama_id: Some("qwen2.5-coder:14b"),
        lmstudio_repo: Some("lmstudio-community/Qwen2.5-Coder-14B-Instruct-GGUF"),
        recommended_quant: "Q4_K_M",
        min_vram_gb: 12,
        strengths: &[ModelStrength::Coding, ModelStrength::InstructionFollowing],
        notes: "Default recommendation for author personas on a single 16GB card.",
    },
    SupportedModel {
        key: "qwen2.5-coder-32b",
        family: "qwen2.5-coder",
        display_name: "Qwen2.5-Coder 32B Instruct",
        params_b: 32.0,
        context_window: 32_768,
        ollama_id: Some("qwen2.5-coder:32b"),
        lmstudio_repo: Some("lmstudio-community/Qwen2.5-Coder-32B-Instruct-GGUF"),
        recommended_quant: "Q4_K_M",
        min_vram_gb: 24,
        strengths: &[ModelStrength::Coding, ModelStrength::InstructionFollowing],
        notes: "Strongest open-weight coder in this size class for the personas that emit JSON contracts.",
    },
    SupportedModel {
        key: "qwen3-14b",
        family: "qwen3",
        display_name: "Qwen3 14B Instruct",
        params_b: 14.0,
        context_window: 131_072,
        ollama_id: Some("qwen3:14b"),
        lmstudio_repo: Some("lmstudio-community/Qwen3-14B-Instruct-GGUF"),
        recommended_quant: "Q4_K_M",
        min_vram_gb: 12,
        strengths: &[
            ModelStrength::GeneralReasoning,
            ModelStrength::Coding,
            ModelStrength::LongContext,
        ],
        notes: "Pick when slices need broader reasoning than pure coding; 128k window for big evidence bundles.",
    },
    SupportedModel {
        key: "qwen3-32b",
        family: "qwen3",
        display_name: "Qwen3 32B Instruct",
        params_b: 32.0,
        context_window: 131_072,
        ollama_id: Some("qwen3:32b"),
        lmstudio_repo: Some("lmstudio-community/Qwen3-32B-Instruct-GGUF"),
        recommended_quant: "Q4_K_M",
        min_vram_gb: 24,
        strengths: &[
            ModelStrength::GeneralReasoning,
            ModelStrength::Coding,
            ModelStrength::LongContext,
        ],
        notes: "Reviewer-grade reasoning. Heavy but reliable for adversarial QA and refactor work.",
    },
    SupportedModel {
        key: "deepseek-coder-v2-lite-16b",
        family: "deepseek-coder-v2",
        display_name: "DeepSeek-Coder-V2-Lite 16B (MoE, 2.4B active)",
        params_b: 16.0,
        context_window: 163_840,
        ollama_id: Some("deepseek-coder-v2:16b"),
        lmstudio_repo: Some("lmstudio-community/DeepSeek-Coder-V2-Lite-Instruct-GGUF"),
        recommended_quant: "Q4_K_M",
        min_vram_gb: 12,
        strengths: &[
            ModelStrength::Coding,
            ModelStrength::LongContext,
        ],
        notes: "MoE — 16B params but only ~2.4B active per token. Fast on consumer GPUs with long context.",
    },
    SupportedModel {
        key: "llama3.3-70b-instruct",
        family: "llama3.3",
        display_name: "Llama 3.3 70B Instruct",
        params_b: 70.0,
        context_window: 131_072,
        ollama_id: Some("llama3.3:70b"),
        lmstudio_repo: Some("lmstudio-community/Llama-3.3-70B-Instruct-GGUF"),
        recommended_quant: "Q4_K_M",
        min_vram_gb: 48,
        strengths: &[
            ModelStrength::GeneralReasoning,
            ModelStrength::InstructionFollowing,
            ModelStrength::LongContext,
        ],
        notes: "Top-tier open-weight reasoner. Needs a workstation card or partial CPU offload.",
    },
];

pub fn list_models() -> Vec<&'static SupportedModel> {
    REGISTRY.iter().collect()
}

pub fn list_models_for(provider: InferenceProvider) -> Vec<&'static SupportedModel> {
    REGISTRY
        .iter()
        .filter(|model| match provider {
            InferenceProvider::Ollama => model.ollama_id.is_some(),
            InferenceProvider::LmStudio => model.lmstudio_repo.is_some(),
        })
        .collect()
}

pub fn find_model(key: &str) -> Option<&'static SupportedModel> {
    REGISTRY.iter().find(|model| model.key == key)
}

pub fn resolve_provider_model_id(provider: InferenceProvider, key: &str) -> Option<&'static str> {
    let model = find_model(key)?;
    match provider {
        InferenceProvider::Ollama => model.ollama_id,
        InferenceProvider::LmStudio => model.lmstudio_repo,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_keys_are_unique() {
        let mut keys: Vec<&str> = REGISTRY.iter().map(|model| model.key).collect();
        keys.sort_unstable();
        let original_len = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), original_len, "duplicate registry keys");
    }

    #[test]
    fn every_model_supports_at_least_one_provider() {
        for model in REGISTRY {
            assert!(
                model.ollama_id.is_some() || model.lmstudio_repo.is_some(),
                "model `{}` advertises neither Ollama nor LM Studio id",
                model.key
            );
        }
    }

    #[test]
    fn provider_filters_match_advertised_ids() {
        for model in list_models_for(InferenceProvider::Ollama) {
            assert!(model.ollama_id.is_some());
        }
        for model in list_models_for(InferenceProvider::LmStudio) {
            assert!(model.lmstudio_repo.is_some());
        }
    }

    #[test]
    fn provider_default_endpoints_are_loopback() {
        assert!(
            InferenceProvider::Ollama
                .default_endpoint()
                .starts_with("http://127.0.0.1:")
        );
        assert!(
            InferenceProvider::LmStudio
                .default_endpoint()
                .starts_with("http://127.0.0.1:")
        );
    }

    #[test]
    fn from_host_kind_only_matches_inference_hosts() {
        assert_eq!(
            InferenceProvider::from_host_kind(HostKind::Ollama),
            Some(InferenceProvider::Ollama)
        );
        assert_eq!(
            InferenceProvider::from_host_kind(HostKind::LmStudio),
            Some(InferenceProvider::LmStudio)
        );
        assert!(InferenceProvider::from_host_kind(HostKind::Claude).is_none());
        assert!(InferenceProvider::from_host_kind(HostKind::Codex).is_none());
        assert!(InferenceProvider::from_host_kind(HostKind::Stub).is_none());
    }

    #[test]
    fn resolve_provider_model_id_returns_provider_specific_id() {
        let id = resolve_provider_model_id(InferenceProvider::Ollama, "qwen2.5-coder-14b");
        assert_eq!(id, Some("qwen2.5-coder:14b"));

        let repo = resolve_provider_model_id(InferenceProvider::LmStudio, "qwen2.5-coder-14b");
        assert_eq!(
            repo,
            Some("lmstudio-community/Qwen2.5-Coder-14B-Instruct-GGUF")
        );
    }
}
