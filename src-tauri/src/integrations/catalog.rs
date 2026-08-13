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
                ),
                provider(
                    "microsoft",
                    "Microsoft 365",
                    "Outlook mail and calendar",
                    &["native_oauth"],
                ),
                provider(
                    "gmail",
                    "Gmail",
                    "Mail search, reading, and sending",
                    &["native_oauth"],
                ),
                provider(
                    "github",
                    "GitHub",
                    "Repositories, issues, and pull requests",
                    &["native_oauth"],
                ),
                provider(
                    "linear",
                    "Linear",
                    "Issues and project updates",
                    &["native_oauth"],
                ),
                provider(
                    "sentry",
                    "Sentry",
                    "Errors, projects, and releases",
                    &["native_oauth"],
                ),
                provider(
                    "notion",
                    "Notion",
                    "Pages and workspace knowledge",
                    &["native_oauth"],
                ),
                provider(
                    "google_drive",
                    "Google Drive",
                    "Files and document context",
                    &["native_oauth"],
                ),
                provider(
                    "sharepoint",
                    "SharePoint",
                    "Sites, files, and organization knowledge",
                    &["native_oauth"],
                ),
            ],
        }
    }
}

fn provider(id: &str, name: &str, summary: &str, modes: &[&str]) -> AppProviderDto {
    AppProviderDto {
        id: id.into(),
        name: name.into(),
        capability_summary: summary.into(),
        connection_modes: modes.iter().map(|mode| (*mode).to_owned()).collect(),
        connect_available: false,
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
}
