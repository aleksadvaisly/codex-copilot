use std::sync::Arc;

use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelUpgradeInfo;
use codex_app_server_protocol::ReasoningEffortOption;
use codex_core::config::Config;
use codex_features::Feature;
use codex_login::AuthManager;
use codex_models_manager::collaboration_mode_presets::CollaborationModesConfig;
use codex_models_manager::manager::ModelsManager;
use codex_models_manager::manager::RefreshStrategy;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ReasoningEffortPreset;

pub async fn supported_models_for_config(
    config: &Config,
    auth_manager: Arc<AuthManager>,
    include_hidden: bool,
) -> Vec<Model> {
    ModelsManager::new_with_provider(
        config.codex_home.to_path_buf(),
        auth_manager,
        config.model_catalog.clone(),
        CollaborationModesConfig {
            default_mode_request_user_input: config
                .features
                .enabled(Feature::DefaultModeRequestUserInput),
        },
        config.model_provider.clone(),
    )
    .list_models(RefreshStrategy::OnlineIfUncached)
    .await
    .into_iter()
    .filter(|preset| include_hidden || preset.show_in_picker)
    .map(model_from_preset)
    .collect()
}

fn model_from_preset(preset: ModelPreset) -> Model {
    Model {
        id: preset.id.to_string(),
        model: preset.model.to_string(),
        upgrade: preset.upgrade.as_ref().map(|upgrade| upgrade.id.clone()),
        upgrade_info: preset.upgrade.as_ref().map(|upgrade| ModelUpgradeInfo {
            model: upgrade.id.clone(),
            upgrade_copy: upgrade.upgrade_copy.clone(),
            model_link: upgrade.model_link.clone(),
            migration_markdown: upgrade.migration_markdown.clone(),
        }),
        availability_nux: preset.availability_nux.map(Into::into),
        display_name: preset.display_name.to_string(),
        description: preset.description.to_string(),
        hidden: !preset.show_in_picker,
        supported_reasoning_efforts: reasoning_efforts_from_preset(
            preset.supported_reasoning_efforts,
        ),
        default_reasoning_effort: preset.default_reasoning_effort,
        input_modalities: preset.input_modalities,
        supports_personality: preset.supports_personality,
        additional_speed_tiers: preset.additional_speed_tiers,
        is_default: preset.is_default,
    }
}

fn reasoning_efforts_from_preset(
    efforts: Vec<ReasoningEffortPreset>,
) -> Vec<ReasoningEffortOption> {
    efforts
        .iter()
        .map(|preset| ReasoningEffortOption {
            reasoning_effort: preset.effort,
            description: preset.description.to_string(),
        })
        .collect()
}
