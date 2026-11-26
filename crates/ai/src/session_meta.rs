//! What the agent says about itself and its session, read from the
//! protocol's messages: the options a session has (mode, model, effort...),
//! the `/` commands it offers (Claude Code's own, and the user's skills),
//! how to log in, and who is logged in.
//!
//! Everything is read leniently: what is not understood is left out.

use std::path::PathBuf;

use serde_json::Value;

/// One value of an option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionValue {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
}

/// An option of a session, as `configOptions` lists them: `mode`, `model`,
/// `effort`, `fast`...
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigOption {
    pub id: String,
    pub name: String,
    /// `mode`, `model`, `thought_level`, `model_config`, or none.
    pub category: Option<String>,
    pub current: Option<String>,
    pub values: Vec<OptionValue>,
}

impl ConfigOption {
    /// The name of the value chosen now.
    pub fn current_name(&self) -> Option<&str> {
        let current = self.current.as_deref()?;
        Some(
            self.values
                .iter()
                .find(|value| value.value == current)
                .map_or(current, |value| value.name.as_str()),
        )
    }
}

/// A model as people call it: Claude Code may name the model in use by
/// its id (`claude-opus-5-5`, from a setting) rather than as its list does
/// ("Opus 5.5"). Anything that is not such an id stays as it is.
pub fn readable_model(name: &str) -> String {
    let (id, wide) = match name.strip_suffix("[1m]") {
        Some(id) => (id, true),
        None => (name, false),
    };
    let Some(rest) = id.strip_prefix("claude-") else {
        return name.to_owned();
    };
    let mut family = None;
    let mut version = Vec::new();
    for part in rest.split('-') {
        if part.chars().all(|c| c.is_ascii_digit()) {
            // A date, as in `claude-sonnet-4-5-20250929`, is not the version.
            if part.len() <= 2 {
                version.push(part);
            }
        } else if family.is_none() && part.chars().all(|c| c.is_ascii_alphabetic()) {
            family = Some(part);
        } else {
            return name.to_owned();
        }
    }
    let Some(family) = family else {
        return name.to_owned();
    };
    let mut readable: String = family
        .chars()
        .take(1)
        .flat_map(char::to_uppercase)
        .chain(family.chars().skip(1))
        .collect();
    if !version.is_empty() {
        readable.push(' ');
        readable.push_str(&version.join("."));
    }
    if wide {
        readable.push_str(" (1M)");
    }
    readable
}

/// The options of a `session/new` or `session/load` answer, or of a
/// `config_option_update`: `configOptions`, or the older `modes` as an
/// option called `mode`.
pub fn config_options(message: &Value) -> Vec<ConfigOption> {
    let listed: Vec<ConfigOption> = message["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| {
            Some(ConfigOption {
                id: option["id"].as_str()?.to_owned(),
                name: option["name"]
                    .as_str()
                    .or(option["id"].as_str())?
                    .to_owned(),
                category: option["category"].as_str().map(str::to_owned),
                current: option["currentValue"].as_str().map(str::to_owned),
                values: option["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|value| {
                        Some(OptionValue {
                            value: value["value"].as_str()?.to_owned(),
                            name: value["name"]
                                .as_str()
                                .or(value["value"].as_str())?
                                .to_owned(),
                            description: value["description"]
                                .as_str()
                                .filter(|text| !text.is_empty())
                                .map(str::to_owned),
                        })
                    })
                    .collect(),
            })
        })
        .filter(|option| !option.values.is_empty())
        .collect();
    if !listed.is_empty() {
        return listed;
    }
    let modes: Vec<OptionValue> = message["modes"]["availableModes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|mode| {
            Some(OptionValue {
                value: mode["id"].as_str()?.to_owned(),
                name: mode["name"].as_str().or(mode["id"].as_str())?.to_owned(),
                description: mode["description"].as_str().map(str::to_owned),
            })
        })
        .collect();
    if modes.is_empty() {
        return Vec::new();
    }
    vec![ConfigOption {
        id: "mode".to_owned(),
        name: "Mode".to_owned(),
        category: Some("mode".to_owned()),
        current: message["modes"]["currentModeId"]
            .as_str()
            .map(str::to_owned),
        values: modes,
    }]
}

/// A `/` command the agent offers: Claude Code's own or a skill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    /// What goes after the name, such as `[file]`.
    pub hint: Option<String>,
}

/// The commands of an `available_commands_update`.
pub fn slash_commands(update: &Value) -> Vec<SlashCommand> {
    update["availableCommands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|command| {
            Some(SlashCommand {
                name: command["name"].as_str()?.trim_start_matches('/').to_owned(),
                description: command["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                hint: command["input"]["hint"]
                    .as_str()
                    .filter(|hint| !hint.is_empty())
                    .map(str::to_owned),
            })
        })
        .collect()
}

/// The commands whose name or description hold `query`, the names that
/// start with it first.
pub fn matching_commands<'a>(commands: &'a [SlashCommand], query: &str) -> Vec<&'a SlashCommand> {
    let query = query.to_lowercase();
    let mut found: Vec<(u8, &SlashCommand)> = commands
        .iter()
        .filter_map(|command| {
            let name = command.name.to_lowercase();
            let rank = if name.starts_with(&query) {
                0
            } else if name.contains(&query) {
                1
            } else if command.description.to_lowercase().contains(&query) {
                2
            } else {
                return None;
            };
            Some((rank, command))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    found.into_iter().map(|(_, command)| command).collect()
}

/// A way to log in the agent offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthMethod {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The program and arguments to run in a terminal for it.
    pub terminal: Option<(PathBuf, Vec<String>)>,
}

/// The `authMethods` of an `initialize` answer.
pub fn auth_methods(initialize: &Value) -> Vec<AuthMethod> {
    initialize["authMethods"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|method| {
            let terminal = &method["_meta"]["terminal-auth"];
            Some(AuthMethod {
                id: method["id"].as_str()?.to_owned(),
                name: method["name"]
                    .as_str()
                    .or(method["id"].as_str())?
                    .to_owned(),
                description: method["description"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                terminal: terminal["command"].as_str().map(|command| {
                    (
                        PathBuf::from(command),
                        terminal["args"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|arg| arg.as_str().map(str::to_owned))
                            .collect(),
                    )
                }),
            })
        })
        .collect()
}

/// Who the agent works as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub signed_in: bool,
    /// Such as "Claude Pro" or "Anthropic API key".
    pub label: Option<String>,
    pub email: Option<String>,
    /// `account`, `api_key`, `gateway`...
    pub kind: Option<String>,
}

/// The account of an `auth/status` answer or an `auth/status_update`.
pub fn account(status: &Value) -> Option<Account> {
    let state = status["state"].as_str()?;
    let active = status["accounts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|account| account["active"].as_bool().unwrap_or(true));
    let text = |value: &Value| {
        value
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    Some(Account {
        signed_in: state == "authenticated",
        label: active.and_then(|account| text(&account["label"])),
        email: active.and_then(|account| text(&account["email"])),
        kind: active.and_then(|account| text(&account["_meta"]["claudeCode"]["kind"])),
    })
}

/// Whether an error says the agent needs a login first.
pub fn needs_login(code: i64, message: &str) -> bool {
    code == -32000 || message.to_lowercase().contains("auth")
}

/// How full the conversation is, and what it cost: a `usage_update`.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextUsage {
    /// Tokens the conversation takes now.
    pub used: u64,
    /// The model's context window.
    pub size: u64,
    /// What the session cost so far, in dollars (shown with an API key).
    pub cost: Option<f64>,
}

impl ContextUsage {
    /// How full the context is, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        if self.size == 0 {
            return 0.;
        }
        (self.used as f64 / self.size as f64).clamp(0., 1.) as f32
    }
}

/// The context of a `usage_update`.
pub fn context_usage(update: &Value) -> Option<ContextUsage> {
    Some(ContextUsage {
        used: update["used"].as_u64()?,
        size: update["size"].as_u64()?,
        cost: update["cost"]["amount"].as_f64(),
    })
}

/// Which limit of the plan a window is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LimitKind {
    FiveHours,
    Week,
    /// The week of one model, such as "Opus".
    WeekOf(String),
}

/// One limit of a Claude plan: how much is used and when it starts again.
#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    pub kind: LimitKind,
    /// From 0 to 100.
    pub percent: f32,
    /// When it starts again, in seconds since 1970.
    pub resets: Option<i64>,
}

/// What `/usage` says: the plan, its limits, what the session cost.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanUsage {
    /// Such as "pro".
    pub plan: Option<String>,
    pub limits: Vec<Limit>,
    pub session_cost: Option<f64>,
}

impl PlanUsage {
    pub fn limit(&self, kind: &LimitKind) -> Option<&Limit> {
        self.limits.iter().find(|limit| &limit.kind == kind)
    }

    /// Takes in the limit a turn reported (`_claude/rateLimit` in a
    /// `usage_update`'s `_meta`), as `/usage` would say it now.
    pub fn take_rate_limit(&mut self, info: &Value) {
        let kind = match info["rateLimitType"].as_str() {
            Some("five_hour") => LimitKind::FiveHours,
            Some("seven_day") => LimitKind::Week,
            Some("seven_day_opus") => LimitKind::WeekOf("Opus".to_owned()),
            Some("seven_day_sonnet") => LimitKind::WeekOf("Sonnet".to_owned()),
            _ => return,
        };
        let Some(utilization) = info["utilization"].as_f64() else {
            return;
        };
        // A fraction, as the API's headers give it, or already a percentage.
        let percent = if utilization <= 1. {
            utilization * 100.
        } else {
            utilization
        } as f32;
        let resets = info["resetsAt"].as_i64();
        match self.limits.iter_mut().find(|limit| limit.kind == kind) {
            Some(limit) => {
                limit.percent = percent;
                if resets.is_some() {
                    limit.resets = resets;
                }
            }
            None => self.limits.push(Limit {
                kind,
                percent,
                resets,
            }),
        }
    }
}

/// Reads the text `/usage` answers with, as the adapter writes it:
///
/// ```text
/// > Claude pro subscription usage
/// **5-hour limit** — **74%** · Resets Nov 25, 2:10 PM GMT+1
/// **Weekly · all models** — **86%** · Resets Nov 29, 2:00 AM GMT+1
/// | $0.00 | 0s | 3s |
/// ```
pub fn plan_usage(text: &str, now: i64) -> Option<PlanUsage> {
    let mut usage = PlanUsage::default();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("> Claude ")
            && let Some(plan) = rest.strip_suffix(" subscription usage")
        {
            usage.plan = Some(plan.trim().to_owned());
            continue;
        }
        if usage.session_cost.is_none()
            && let Some(rest) = line.strip_prefix("| $")
        {
            usage.session_cost = rest
                .split('|')
                .next()
                .and_then(|cost| cost.trim().parse().ok());
            continue;
        }
        let Some(rest) = line.strip_prefix("**") else {
            continue;
        };
        let Some((label, rest)) = rest.split_once("** — **") else {
            continue;
        };
        let Some((percent, rest)) = rest.split_once("%**") else {
            continue;
        };
        let Ok(percent) = percent.trim().parse::<f32>() else {
            continue;
        };
        let kind = match label.trim() {
            "5-hour limit" => LimitKind::FiveHours,
            "Weekly · all models" => LimitKind::Week,
            other => match other.strip_prefix("Weekly · ") {
                Some(model) => LimitKind::WeekOf(model.trim().to_owned()),
                None => continue,
            },
        };
        let resets = rest
            .split_once("Resets ")
            .and_then(|(_, when)| reset_time(when.trim(), now));
        usage.limits.push(Limit {
            kind,
            percent,
            resets,
        });
    }
    (usage.plan.is_some() || !usage.limits.is_empty() || usage.session_cost.is_some())
        .then_some(usage)
}

/// "Nov 25, 2:10 PM GMT+1" in seconds since 1970: the next such moment
/// after a day before `now`, as `/usage` writes no year.
fn reset_time(text: &str, now: i64) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut words = text.split_whitespace();
    let name = words.next()?;
    let month = MONTHS.iter().position(|month| *month == name)? as i64 + 1;
    let day: i64 = words.next()?.trim_end_matches(',').parse().ok()?;
    let (hour, minute) = words.next()?.split_once(':')?;
    let (mut hour, minute): (i64, i64) = (hour.parse().ok()?, minute.parse().ok()?);
    match words.next()? {
        "PM" if hour < 12 => hour += 12,
        "AM" if hour == 12 => hour = 0,
        "AM" | "PM" => {}
        _ => return None,
    }
    let offset = match words.next() {
        Some(zone) => {
            let zone = zone
                .strip_prefix("GMT")
                .or_else(|| zone.strip_prefix("UTC"))?;
            if zone.is_empty() {
                0
            } else {
                let sign = if zone.starts_with('-') { -1 } else { 1 };
                let zone = zone.trim_start_matches(['+', '-']);
                let (hours, minutes) = zone.split_once(':').unwrap_or((zone, "0"));
                sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60)
            }
        }
        None => 0,
    };
    let year = civil_year(now);
    let at =
        |year: i64| days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 - offset;
    let moment = at(year);
    Some(if moment < now - 86_400 {
        at(year + 1)
    } else {
        moment
    })
}

/// The year of a moment, in UTC.
fn civil_year(seconds: i64) -> i64 {
    // Howard Hinnant's civil_from_days.
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days - era * 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    year_of_era + era * 400 + i64::from(month <= 2)
}

/// Days since 1970 of a date (Howard Hinnant's days_from_civil).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {

    #[test]
    fn model_ids_read_as_names() {
        assert_eq!(readable_model("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(readable_model("claude-fable-5-2"), "Fable 5.2");
        assert_eq!(readable_model("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(readable_model("claude-3-5-haiku-20241022"), "Haiku 3.5");
        assert_eq!(readable_model("claude-opus-5-5[1m]"), "Opus 5.5 (1M)");
        // Names, aliases and other providers' models stay as they are.
        assert_eq!(readable_model("Opus 5.5"), "Opus 5.5");
        assert_eq!(readable_model("opus"), "opus");
        assert_eq!(
            readable_model("qwen/qwen-2.5-coder-32b-instruct"),
            "qwen/qwen-2.5-coder-32b-instruct"
        );
    }

    use serde_json::json;

    use super::*;

    #[test]
    fn options_from_config_options_or_the_older_modes() {
        let session = json!({ "configOptions": [
            { "id": "mode", "name": "Mode", "category": "mode", "currentValue": "auto",
              "options": [{ "value": "default", "name": "Manual" }, { "value": "auto", "name": "Auto" }] },
            { "id": "model", "name": "Model", "category": "model", "currentValue": "opus",
              "options": [{ "value": "opus", "name": "Opus 5.5", "description": "The most capable" }] },
            { "id": "empty", "name": "Nothing", "options": [] },
        ]});
        let options = config_options(&session);
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].current_name(), Some("Auto"));
        assert_eq!(
            options[1].values[0].description.as_deref(),
            Some("The most capable")
        );
        let older = json!({ "modes": { "currentModeId": "plan", "availableModes": [
            { "id": "default", "name": "Default" }, { "id": "plan", "name": "Plan" }] } });
        let options = config_options(&older);
        assert_eq!(options[0].id, "mode");
        assert_eq!(options[0].current_name(), Some("Plan"));
    }

    #[test]
    fn commands_and_how_a_query_finds_them() {
        let update = json!({ "availableCommands": [
            { "name": "review", "description": "Review the changes" },
            { "name": "compact", "description": "Make the conversation shorter", "input": { "hint": "[instructions]" } },
            { "name": "frontend-design", "description": "Design pages that look designed" },
        ]});
        let commands = slash_commands(&update);
        assert_eq!(commands[1].hint.as_deref(), Some("[instructions]"));
        let names = |query: &str| -> Vec<String> {
            matching_commands(&commands, query)
                .iter()
                .map(|command| command.name.clone())
                .collect()
        };
        assert_eq!(names("re"), ["review"]);
        assert_eq!(names("e"), ["frontend-design", "review", "compact"]);
        assert_eq!(names("design"), ["frontend-design"]);
        assert_eq!(names("shorter"), ["compact"]);
        assert_eq!(names("").len(), 3);
    }

    #[test]
    fn login_methods_and_the_account() {
        let initialize = json!({ "authMethods": [{ "id": "claude-ai-login", "name": "Claude Subscription",
            "description": "Use Claude subscription ", "type": "terminal", "args": ["--cli", "auth", "login"],
            "_meta": { "terminal-auth": { "command": "/usr/bin/node", "args": ["/a/claude-agent-acp", "--cli", "auth", "login"] } } }] });
        let methods = auth_methods(&initialize);
        assert_eq!(methods[0].description, "Use Claude subscription");
        let (program, args) = methods[0].terminal.clone().unwrap();
        assert_eq!(program, PathBuf::from("/usr/bin/node"));
        assert_eq!(args.len(), 4);

        let status = json!({ "state": "authenticated", "accounts": [{ "label": "Claude Pro", "active": true,
            "email": "someone@example.com", "_meta": { "claudeCode": { "kind": "account" } } }] });
        let signed = account(&status).unwrap();
        assert!(signed.signed_in);
        assert_eq!(signed.label.as_deref(), Some("Claude Pro"));
        assert_eq!(signed.kind.as_deref(), Some("account"));
        let out = account(&json!({ "state": "unauthenticated", "accounts": [] })).unwrap();
        assert!(!out.signed_in && out.email.is_none());
        assert!(account(&json!({})).is_none());
    }

    #[test]
    fn the_plan_and_its_limits_from_usage() {
        // 25 Nov 2026, 10:00 in Rome (09:00 UTC).
        let now = days_from_civil(2026, 11, 25) * 86_400 + 9 * 3600;
        let text = "## Usage\n\n> Claude pro subscription usage\n\n### Limits\n\n\
            **5-hour limit** — **74%** · Resets Nov 25, 2:10 PM GMT+1\n\n\
            **Weekly · all models** — **86%** · Resets Nov 29, 2:00 AM GMT+1\n\n\
            **Weekly · Opus** — **12.5%**\n\n| Cost | API time | Active |\n|:--|:--|:--|\n| $1.25 | 0s | 3s |\n";
        let usage = plan_usage(text, now).unwrap();
        assert_eq!(usage.plan.as_deref(), Some("pro"));
        assert_eq!(usage.session_cost, Some(1.25));
        let five = usage.limit(&LimitKind::FiveHours).unwrap();
        assert_eq!(five.percent, 74.);
        // 14:10 in Rome is 13:10 UTC, four hours and ten minutes from now.
        assert_eq!(five.resets, Some(now + 4 * 3600 + 600));
        let week = usage.limit(&LimitKind::Week).unwrap();
        assert_eq!(
            week.resets,
            Some(days_from_civil(2026, 11, 29) * 86_400 + 3600)
        );
        let opus = usage.limit(&LimitKind::WeekOf("Opus".to_owned())).unwrap();
        assert_eq!((opus.percent, opus.resets), (12.5, None));
        // January, read in December, is next year's.
        let december = days_from_civil(2026, 12, 30) * 86_400;
        assert_eq!(
            reset_time("Jan 2, 12:00 AM GMT", december),
            Some(days_from_civil(2027, 1, 2) * 86_400)
        );
        assert!(plan_usage("nothing here", now).is_none());

        // A turn's own report moves the bar.
        let mut usage = usage;
        usage.take_rate_limit(
            &json!({ "rateLimitType": "five_hour", "utilization": 0.8, "resetsAt": 5 }),
        );
        let five = usage.limit(&LimitKind::FiveHours).unwrap();
        assert_eq!((five.percent.round(), five.resets), (80., Some(5)));

        let context = context_usage(&json!({ "sessionUpdate": "usage_update", "used": 250_000,
            "size": 1_000_000, "cost": { "amount": 0.42, "currency": "USD" } }))
        .unwrap();
        assert_eq!((context.fraction(), context.cost), (0.25, Some(0.42)));
    }
}
