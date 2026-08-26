//! Auth-surface evidence for native Gemini.
//!
//! Google ships four *distinct* Gemini entry points. They are not
//! interchangeable, they do not share a credential, and they do not share a
//! billing owner. Plan 038 requires Alfred to select exactly one and to say so
//! out loud, so the boundary lives here as data rather than as prose in a
//! settings string.

use serde::Serialize;

/// A documented Gemini entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GeminiAuthSurface {
    /// A user-supplied Gemini API auth key sent as `x-goog-api-key` to
    /// `generativelanguage.googleapis.com`.
    ApiKey,
    /// Google OAuth for an installed desktop app / Application Default
    /// Credentials for the Generative Language API.
    GoogleOauthDesktop,
    /// Vertex AI on `aiplatform.googleapis.com`, scoped to a Cloud project.
    VertexAi,
    /// The consumer Gemini app plan and the Gemini Code Assist licence that
    /// Gemini CLI's Google-account login grants.
    ConsumerSubscription,
}

/// Whether Alfred's native harness may use a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GeminiSurfaceStatus {
    /// The surface the native runtime implements.
    Selected,
    /// Documented, but Alfred cannot reach it without an unresolved gate.
    Blocked,
}

/// Everything a reviewer or a settings pane needs in order to tell the four
/// surfaces apart. Every field is a documented fact, never an estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiSurfaceEvidence {
    pub surface: GeminiAuthSurface,
    pub status: GeminiSurfaceStatus,
    /// Stable snake_case identifier. For a blocked surface this is the exact
    /// capability reason Plan 038 records.
    pub code: &'static str,
    pub label: &'static str,
    /// Who is invoiced for a native turn on this surface.
    pub billing_owner: &'static str,
    /// Project / region obligations, or why none apply.
    pub project_region: &'static str,
    /// How the model catalog is obtained.
    pub models: &'static str,
    /// Whether an authoritative remaining-quota reading exists.
    pub quota: &'static str,
    /// What Google does with prompts and responses.
    pub data_handling: &'static str,
    /// The primary official reference re-checked for this plan.
    pub reference: &'static str,
}

/// The four surfaces, in the order Plan 038 evaluates them.
pub const GEMINI_AUTH_SURFACES: [GeminiSurfaceEvidence; 4] = [
    GeminiSurfaceEvidence {
        surface: GeminiAuthSurface::ApiKey,
        status: GeminiSurfaceStatus::Selected,
        code: "gemini_api_auth_key",
        label: "Gemini API auth key",
        billing_owner: "The Google Cloud project that owns a standard key, or the project \
             and service-account binding behind an authorization key. Free tier \
             bills nobody; a linked billing account invoices that account.",
        project_region: "No region is chosen by the client. The key's own project is the \
             only project involved, and Google restricts free-tier use for end \
             users in the EEA, Switzerland, and the UK to paid services.",
        models: "Listed live from GET /v1beta/models, filtered to models that \
             advertise the generateContent method.",
        quota: "Unavailable. Google publishes no remaining-quota endpoint for this \
             surface; limits are project-scoped and read in AI Studio, while \
             authorization-key requests are not recorded in Cloud service-account \
             usage metrics. Alfred therefore reports usage as unavailable instead \
             of estimating it.",
        data_handling: "Unpaid tier: Google uses submitted content and responses to improve \
             its products and human reviewers may read them. Paid tier: prompts \
             and responses are not used for product improvement. In the EEA, \
             Switzerland, and the UK, paid-data terms apply even to unpaid quota, \
             but an API client made available there must use Paid Services.",
        reference: "https://ai.google.dev/gemini-api/docs/api-key",
    },
    GeminiSurfaceEvidence {
        surface: GeminiAuthSurface::GoogleOauthDesktop,
        status: GeminiSurfaceStatus::Blocked,
        code: "gemini_oauth_client_packaging_unresolved",
        label: "Google OAuth desktop client / ADC",
        billing_owner: "The Cloud project behind the OAuth client, which Alfred would have \
             to own or ask each user to create.",
        project_region: "Requires an OAuth client registered in a Cloud project. Google's \
             installed-app sample downloads desktop client configuration with \
             client ID and client_secret fields and writes ADC/token files.",
        models: "Same catalog as the API-key surface once authorized.",
        quota: "Unavailable; same absence of a remaining-quota endpoint.",
        data_handling: "Same Gemini API terms as the API-key surface.",
        // Blocked: Google documents the installed-app flow as a testing-grade
        // approach. Alfred has no registered/verified desktop public-client
        // configuration or packaged redirect flow, and the sample's
        // cloud-platform scope is broader than one agent turn needs. Plan 038
        // stops rather than treating a sample client_secret field as a
        // confidential desktop secret or borrowing an ADC file.
        reference: "https://ai.google.dev/gemini-api/docs/oauth",
    },
    GeminiSurfaceEvidence {
        surface: GeminiAuthSurface::VertexAi,
        status: GeminiSurfaceStatus::Blocked,
        code: "gemini_vertex_project_binding_unresolved",
        label: "Vertex AI (aiplatform.googleapis.com)",
        billing_owner: "The Google Cloud billing account attached to the caller's Vertex \
             project. Never Alfred, and never a consumer Gemini plan.",
        project_region: "Standard Vertex calls name a project and location and use ADC, a \
             service account, or an authorization key. Vertex Express Mode is \
             a separate Preview API-key onboarding path without a client-chosen \
             project/location. Alfred has selected neither Vertex account shape.",
        models: "Vertex publisher model catalog for the selected standard \
             project/location, or the separate Express Mode catalog.",
        quota: "Unavailable to the client; Vertex quota is read through Cloud \
             Monitoring and the Cloud console.",
        data_handling: "Google Cloud Platform terms and the Cloud privacy notice.",
        // Blocked: standard onboarding requires an ADC/authorization-key
        // bootstrap plus project/region/billing selection; Express Mode is a
        // distinct Preview API-key product. The Plan 031 account schema has no
        // place to distinguish or record these. Guessing would misstate the
        // endpoint, identity, region, and billing owner.
        reference: "https://docs.cloud.google.com/vertex-ai/docs/authentication",
    },
    GeminiSurfaceEvidence {
        surface: GeminiAuthSurface::ConsumerSubscription,
        status: GeminiSurfaceStatus::Blocked,
        code: "gemini_consumer_subscription_prohibited",
        label: "Consumer Gemini plan / Gemini CLI Google-account login",
        billing_owner: "The end user's consumer Google subscription. It grants no \
             third-party API entitlement of any kind.",
        project_region: "Not applicable; there is no client-visible project.",
        models: "Not applicable.",
        quota: "Not applicable.",
        data_handling: "Google Terms of Service and the Gemini Code Assist for individuals \
             privacy notice.",
        // Blocked: Gemini CLI's own terms state that directly accessing the
        // services powering Gemini CLI (for example the Gemini Code Assist
        // service) with third-party software is a violation of applicable
        // terms and may end the user's account. Alfred never reads a Gemini CLI
        // credential and never presents API or Vertex usage as subscription
        // usage.
        reference:
            "https://github.com/google-gemini/gemini-cli/blob/main/docs/resources/tos-privacy.md",
    },
];

/// The surface the native runtime actually implements.
pub const SELECTED_SURFACE: GeminiAuthSurface = GeminiAuthSurface::ApiKey;

impl GeminiAuthSurface {
    pub fn evidence(self) -> &'static GeminiSurfaceEvidence {
        GEMINI_AUTH_SURFACES
            .iter()
            .find(|entry| entry.surface == self)
            .expect("every Gemini auth surface has evidence")
    }
}

/// Blocked capability reasons, in the exact wording Plan 038 records.
pub fn blocked_surface_codes() -> Vec<&'static str> {
    GEMINI_AUTH_SURFACES
        .iter()
        .filter(|entry| entry.status == GeminiSurfaceStatus::Blocked)
        .map(|entry| entry.code)
        .collect()
}
