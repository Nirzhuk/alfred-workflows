use std::collections::HashMap;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

use super::models::LicenseProduct;

const PRODUCTION_API_BASE: &str = "https://api.polar.sh";
const SANDBOX_API_BASE: &str = "https://sandbox-api.polar.sh";
const ALLOWED_HOSTS: [&str; 2] = ["api.polar.sh", "sandbox-api.polar.sh"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolarEnvironment {
    Production,
    Sandbox,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PolarConfigError {
    #[error("Polar configuration is incomplete")]
    Incomplete,
    #[error("Polar environment is invalid")]
    InvalidEnvironment,
    #[error("Polar identifier is invalid")]
    InvalidIdentifier,
    #[error("Polar API base is not allowed")]
    InvalidApiBase,
}

impl PolarConfigError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Incomplete => "polar_config_incomplete",
            Self::InvalidEnvironment => "polar_environment_invalid",
            Self::InvalidIdentifier => "polar_identifier_invalid",
            Self::InvalidApiBase => "polar_api_base_invalid",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PolarConfig {
    pub api_base: Url,
    pub organization_id: Uuid,
    benefits: HashMap<Uuid, LicenseProduct>,
}

impl PolarConfig {
    /// Reads the four public Polar values that the publisher bakes into a
    /// build. They come from the workspace `.env` through `build.rs`, the same
    /// reviewed place as every other `ALFRED_*` publisher value, so binding a
    /// sandbox is a one-file edit and a rebuild. A source build with no `.env`
    /// stays unconfigured instead of failing.
    pub fn load() -> Result<Option<Self>, PolarConfigError> {
        Self::resolve([
            option_env!("ALFRED_POLAR_ENVIRONMENT"),
            option_env!("ALFRED_POLAR_ORGANIZATION_ID"),
            option_env!("ALFRED_POLAR_INDIVIDUAL_BENEFIT_ID"),
            option_env!("ALFRED_POLAR_TEAMS_BENEFIT_ID"),
        ])
    }

    /// `Ok(None)` when nothing is bound, `Err(Incomplete)` when only some of
    /// the *required* values are, so a half-filled `.env` is reported rather
    /// than silently running unlicensed. A key present but blank counts as
    /// unset, because `.env` templates ship with empty values.
    ///
    /// The Teams benefit is **optional**: Polar manages seats natively, and a
    /// publisher who does not sell the seat-based product has no such benefit
    /// to bind. Leaving it unset simply means no benefit maps to
    /// `LicenseProduct::Teams`.
    pub(crate) fn resolve(values: [Option<&str>; 4]) -> Result<Option<Self>, PolarConfigError> {
        let values = values.map(|value| value.map(str::trim).filter(|value| !value.is_empty()));

        if values.iter().all(Option::is_none) {
            return Ok(None);
        }

        let [environment, organization, individual, teams] = values;
        if [environment, organization, individual]
            .iter()
            .any(Option::is_none)
        {
            return Err(PolarConfigError::Incomplete);
        }

        let environment = match environment {
            Some("production") => PolarEnvironment::Production,
            Some("sandbox") => PolarEnvironment::Sandbox,
            _ => return Err(PolarConfigError::InvalidEnvironment),
        };

        Self::new(
            environment,
            organization.unwrap_or_default(),
            individual.unwrap_or_default(),
            teams,
        )
        .map(Some)
    }

    pub fn new(
        environment: PolarEnvironment,
        organization_id: &str,
        individual_benefit_id: &str,
        teams_benefit_id: Option<&str>,
    ) -> Result<Self, PolarConfigError> {
        let api_base = match environment {
            PolarEnvironment::Production => PRODUCTION_API_BASE,
            PolarEnvironment::Sandbox => SANDBOX_API_BASE,
        };
        Self::from_parts(
            environment,
            api_base,
            organization_id,
            individual_benefit_id,
            teams_benefit_id,
        )
    }

    fn from_parts(
        environment: PolarEnvironment,
        api_base: &str,
        organization_id: &str,
        individual_benefit_id: &str,
        teams_benefit_id: Option<&str>,
    ) -> Result<Self, PolarConfigError> {
        let api_base = Url::parse(api_base).map_err(|_| PolarConfigError::InvalidApiBase)?;
        let host = api_base
            .host_str()
            .ok_or(PolarConfigError::InvalidApiBase)?;
        if api_base.scheme() != "https"
            || !ALLOWED_HOSTS.contains(&host)
            || api_base.path() != "/"
            || api_base.query().is_some()
            || api_base.fragment().is_some()
            || (environment == PolarEnvironment::Production && host != "api.polar.sh")
            || (environment == PolarEnvironment::Sandbox && host != "sandbox-api.polar.sh")
        {
            return Err(PolarConfigError::InvalidApiBase);
        }

        let organization_id = parse_v4(organization_id)?;
        let mut benefits = HashMap::new();
        for (id, product) in [
            (Some(individual_benefit_id), LicenseProduct::Individual),
            (teams_benefit_id, LicenseProduct::Teams),
        ] {
            let Some(id) = id else { continue };
            let id = parse_v4(id)?;
            if benefits.insert(id, product).is_some() {
                return Err(PolarConfigError::InvalidIdentifier);
            }
        }

        Ok(Self {
            api_base,
            organization_id,
            benefits,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(api_base: Url) -> Self {
        let benefits = HashMap::from([
            (
                Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap(),
                LicenseProduct::Individual,
            ),
            (
                Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
                LicenseProduct::Teams,
            ),
        ]);
        Self {
            api_base,
            organization_id: Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
            benefits,
        }
    }

    pub fn product_for_benefit(&self, benefit_id: &str) -> Option<LicenseProduct> {
        Uuid::parse_str(benefit_id)
            .ok()
            .and_then(|id| self.benefits.get(&id).copied())
    }
}

fn parse_v4(value: &str) -> Result<Uuid, PolarConfigError> {
    let id = Uuid::parse_str(value).map_err(|_| PolarConfigError::InvalidIdentifier)?;
    if id.get_version_num() != 4 {
        return Err(PolarConfigError::InvalidIdentifier);
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const INDIVIDUAL: &str = "11111111-1111-4111-8111-111111111111";
    const TEAMS: &str = "33333333-3333-4333-8333-333333333333";
    /// A benefit that belongs to no configured product, standing in for the
    /// retired third class.
    const UNBOUND: &str = "22222222-2222-4222-8222-222222222222";

    fn bound(values: [Option<&str>; 4]) -> Result<Option<PolarConfig>, PolarConfigError> {
        PolarConfig::resolve(values)
    }

    #[test]
    fn an_unconfigured_source_build_stays_unconfigured_instead_of_failing() {
        assert!(bound([None; 4]).expect("no binding").is_none());
        // `.env.example` ships the keys with empty values; that is still unset.
        assert!(bound([Some(""), Some("  "), Some(""), Some("")])
            .expect("blank binding")
            .is_none());
    }

    #[test]
    fn a_partially_configured_build_reports_the_gap() {
        assert_eq!(
            bound([None, Some(ORG), Some(INDIVIDUAL), Some(TEAMS)]).unwrap_err(),
            PolarConfigError::Incomplete
        );
        assert_eq!(
            bound([Some("sandbox"), Some(ORG), Some("   "), Some(TEAMS)]).unwrap_err(),
            PolarConfigError::Incomplete
        );
        assert_eq!(
            bound([Some("staging"), Some(ORG), Some(INDIVIDUAL), Some(TEAMS)]).unwrap_err(),
            PolarConfigError::InvalidEnvironment
        );
    }

    #[test]
    fn the_teams_benefit_is_optional_because_polar_manages_seats() {
        // A publisher who sells no seat-based product has no such benefit to
        // bind; that is a complete configuration, not a half-filled one.
        let config = bound([Some("sandbox"), Some(ORG), Some(INDIVIDUAL), None])
            .expect("seatless binding")
            .expect("configured");

        assert_eq!(
            config.product_for_benefit(INDIVIDUAL),
            Some(LicenseProduct::Individual)
        );
        // Nothing maps to Teams, and no placeholder stands in for it.
        assert_eq!(config.product_for_benefit(TEAMS), None);

        // A blank value in `.env` is the same as absent.
        assert!(bound([Some("sandbox"), Some(ORG), Some(INDIVIDUAL), Some("  ")])
            .expect("blank seat binding")
            .is_some());

        // The other three stay required.
        assert_eq!(
            bound([Some("sandbox"), Some(ORG), None, None]).unwrap_err(),
            PolarConfigError::Incomplete
        );
    }

    #[test]
    fn a_fully_configured_build_maps_exactly_two_benefits() {
        let config = bound([Some("sandbox"), Some(ORG), Some(INDIVIDUAL), Some(TEAMS)])
            .expect("sandbox binding")
            .expect("configured");

        assert_eq!(config.api_base.as_str(), "https://sandbox-api.polar.sh/");
        assert_eq!(config.organization_id.to_string(), ORG);
        assert_eq!(
            config.product_for_benefit(INDIVIDUAL),
            Some(LicenseProduct::Individual)
        );
        assert_eq!(
            config.product_for_benefit(TEAMS),
            Some(LicenseProduct::Teams)
        );
        // A third benefit class no longer exists to bind.
        assert_eq!(config.product_for_benefit(UNBOUND), None);

        // Surrounding whitespace in a hand-edited `.env` must not break binding.
        assert!(bound([
            Some(" production "),
            Some(" aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa "),
            Some(INDIVIDUAL),
            Some(TEAMS),
        ])
        .expect("trimmed binding")
        .is_some());
    }

    #[test]
    fn production_and_sandbox_use_only_their_allowlisted_https_hosts() {
        let production =
            PolarConfig::new(PolarEnvironment::Production, ORG, INDIVIDUAL, Some(TEAMS))
                .expect("production config");
        let sandbox = PolarConfig::new(PolarEnvironment::Sandbox, ORG, INDIVIDUAL, Some(TEAMS))
            .expect("sandbox config");
        assert_eq!(production.api_base.as_str(), "https://api.polar.sh/");
        assert_eq!(sandbox.api_base.as_str(), "https://sandbox-api.polar.sh/");

        for base in [
            "http://api.polar.sh",
            "https://api.polar.sh.attacker.invalid",
            "https://sandbox-api.polar.sh",
        ] {
            assert_eq!(
                PolarConfig::from_parts(
                    PolarEnvironment::Production,
                    base,
                    ORG,
                    INDIVIDUAL,
                    Some(TEAMS),
                )
                .unwrap_err(),
                PolarConfigError::InvalidApiBase,
                "{base}"
            );
        }
    }

    #[test]
    fn requires_distinct_uuid_v4_identifiers_and_maps_configured_benefits() {
        let config = PolarConfig::new(PolarEnvironment::Production, ORG, INDIVIDUAL, Some(TEAMS))
            .expect("valid config");
        assert_eq!(
            config.product_for_benefit(INDIVIDUAL),
            Some(LicenseProduct::Individual)
        );
        assert_eq!(
            config.product_for_benefit(TEAMS),
            Some(LicenseProduct::Teams)
        );
        assert_eq!(
            config.product_for_benefit("44444444-4444-4444-8444-444444444444"),
            None
        );

        assert_eq!(
            PolarConfig::new(
                PolarEnvironment::Production,
                ORG,
                INDIVIDUAL,
                Some(INDIVIDUAL),
            )
            .unwrap_err(),
            PolarConfigError::InvalidIdentifier
        );
        assert_eq!(
            PolarConfig::new(
                PolarEnvironment::Production,
                "not-a-uuid",
                INDIVIDUAL,
                Some(TEAMS),
            )
            .unwrap_err(),
            PolarConfigError::InvalidIdentifier
        );
    }
}
