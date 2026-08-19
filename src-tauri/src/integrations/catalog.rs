use super::models::AppProviderDto;

#[derive(Debug, Clone)]
pub struct ProviderCatalog {
    providers: Vec<AppProviderDto>,
}

impl Default for ProviderCatalog {
    fn default() -> Self {
        Self {
            providers: vec![
                provider(
                    "slack",
                    "Slack",
                    "Messages and channel activity",
                    &["native_oauth", "private_bot"],
                    true,
                ),
                provider(
                    "telegram",
                    "Telegram",
                    "Send plain-text notifications to your paired private chat",
                    &["private_bot"],
                    true,
                ),
                AppProviderDto {
                    experimental: true,
                    single_connection: true,
                    ..provider(
                        "whatsapp",
                        "WhatsApp",
                        "Send plain-text notifications to your own WhatsApp chat while Alfred is running",
                        &["linked_device_experimental"],
                        super::whatsapp::provider::is_available(),
                    )
                },
                provider(
                    "microsoft",
                    "Microsoft 365",
                    "Outlook mail and calendar",
                    &["native_oauth"],
                    false,
                ),
                provider(
                    "gmail",
                    "Gmail",
                    "Mail search, reading, and sending",
                    &["native_oauth"],
                    false,
                ),
                provider(
                    "github",
                    "GitHub",
                    "Repositories, issues, and pull requests",
                    &["github_app_device"],
                    super::github::is_configured(),
                ),
                provider(
                    "linear",
                    "Linear",
                    "Issues and project updates",
                    &["native_oauth"],
                    false,
                ),
                provider(
                    "sentry",
                    "Sentry",
                    "Errors, projects, and releases",
                    &["native_oauth"],
                    false,
                ),
                provider(
                    "notion",
                    "Notion",
                    "Selected pages and data sources, fetched on demand",
                    &["private_bot", "native_oauth"],
                    true,
                ),
                provider(
                    "obsidian",
                    "Obsidian",
                    "Markdown notes from one local vault, read on demand",
                    &["local_vault"],
                    true,
                ),
                provider(
                    "google_drive",
                    "Google Drive",
                    "Files and document context",
                    &["native_oauth"],
                    false,
                ),
                provider(
                    "sharepoint",
                    "SharePoint",
                    "Sites, files, and organization knowledge",
                    &["native_oauth"],
                    false,
                ),
            ],
        }
    }
}

fn provider(
    id: &str,
    name: &str,
    summary: &str,
    modes: &[&str],
    connect_available: bool,
) -> AppProviderDto {
    AppProviderDto {
        id: id.into(),
        name: name.into(),
        capability_summary: summary.into(),
        connection_modes: modes.iter().map(|mode| (*mode).to_owned()).collect(),
        connect_available,
        experimental: false,
        single_connection: false,
    }
}

impl ProviderCatalog {
    pub fn list(&self) -> Vec<AppProviderDto> {
        self.providers.clone()
    }

    pub fn contains(&self, provider_id: &str) -> bool {
        self.providers
            .iter()
            .any(|provider| provider.id == provider_id)
    }

    pub fn get(&self, provider_id: &str) -> Option<&AppProviderDto> {
        self.providers
            .iter()
            .find(|provider| provider.id == provider_id)
    }

    /// Whether this provider allows at most one linked account.
    pub fn is_single_connection(&self, provider_id: &str) -> bool {
        self.get(provider_id)
            .is_some_and(|provider| provider.single_connection)
    }
}
