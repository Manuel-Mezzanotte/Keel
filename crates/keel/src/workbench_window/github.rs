//! GitHub through its own program, `gh`, with the account it is signed in
//! with: Keel never sees a token. Here are the commands and the reading
//! of their answers; `pull_requests.rs` shows them.

use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

/// What lists and details ask about a pull request.
const LIST_FIELDS: &str = "number,title,author,headRefName,baseRefName,isDraft,state,updatedAt,reviewDecision,statusCheckRollup,url";
const DETAIL_FIELDS: &str = "number,title,author,headRefName,baseRefName,isDraft,state,updatedAt,reviewDecision,statusCheckRollup,url,body,headRefOid,baseRefOid,mergeable,additions,deletions,files,reviews,comments";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CheckState {
    Pending,
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Check {
    pub(super) name: String,
    pub(super) workflow: Option<String>,
    pub(super) state: CheckState,
    pub(super) url: Option<String>,
    /// The job of GitHub Actions, to read its log.
    pub(super) job: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PullState {
    Open,
    Closed,
    Merged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Decision {
    None,
    Required,
    Approved,
    Changes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pull {
    pub(super) number: u64,
    pub(super) title: String,
    pub(super) author: String,
    pub(super) head: String,
    pub(super) base: String,
    pub(super) draft: bool,
    pub(super) state: PullState,
    /// In seconds since 1970.
    pub(super) updated: i64,
    pub(super) decision: Decision,
    pub(super) checks: Vec<Check>,
    pub(super) url: String,
}

/// Something said in the conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Said {
    pub(super) author: String,
    pub(super) body: String,
    pub(super) at: i64,
    /// `APPROVED`, `CHANGES_REQUESTED`… for a review.
    pub(super) review: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PullFile {
    pub(super) path: String,
    pub(super) additions: u64,
    pub(super) deletions: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Details {
    pub(super) body: String,
    pub(super) head_oid: String,
    pub(super) base_oid: String,
    /// `None` while GitHub works it out.
    pub(super) mergeable: Option<bool>,
    pub(super) additions: u64,
    pub(super) deletions: u64,
    pub(super) files: Vec<PullFile>,
    pub(super) talk: Vec<Said>,
}

/// A comment on a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Comment {
    pub(super) id: u64,
    pub(super) path: String,
    /// From 1, in the newer version; `None` when the line is gone.
    pub(super) line: Option<u32>,
    /// On the older version.
    pub(super) left: bool,
    pub(super) author: String,
    pub(super) body: String,
    pub(super) at: i64,
    pub(super) reply_to: Option<u64>,
}

/// The comments on a line, the first and its replies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Thread {
    pub(super) path: String,
    pub(super) line: Option<u32>,
    pub(super) left: bool,
    pub(super) comments: Vec<Comment>,
}

/// Which pull requests are listed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Filter {
    #[default]
    Open,
    Mine,
    ToReview,
    Closed,
}

impl Filter {
    pub(super) const ALL: [Filter; 4] =
        [Filter::Open, Filter::Mine, Filter::ToReview, Filter::Closed];

    fn args(self) -> Vec<&'static str> {
        match self {
            Filter::Open => vec!["--state", "open"],
            Filter::Mine => vec!["--state", "open", "--author", "@me"],
            Filter::ToReview => vec!["--state", "open", "--search", "review-requested:@me"],
            Filter::Closed => vec!["--state", "closed"],
        }
    }
}

/// How a review ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}

/// The seconds since 1970 of `2026-11-19T18:08:12Z`.
fn seconds(text: &str) -> i64 {
    let number = |range: std::ops::Range<usize>| -> Option<i64> { text.get(range)?.parse().ok() };
    let read = || -> Option<i64> {
        let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
        let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
        // Days from 1970, with years that start in March.
        let year = if month <= 2 { year - 1 } else { year };
        let era = year.div_euclid(400);
        let of_era = year - era * 400;
        let of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
        let days = era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468;
        Some(days * 86_400 + hour * 3600 + minute * 60 + second)
    };
    read().unwrap_or_default()
}

/// A text for reading: without hidden comments, and with pictures as
/// links, so nothing is loaded from elsewhere by looking at it.
pub(super) fn readable(text: &str) -> String {
    static HIDDEN: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?s)<!--.*?-->").expect("comments"));
    static PICTURE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"!\[([^\]]*)\]").expect("pictures"));
    static TAG: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?is)</?(?:img|picture|source|video|iframe)\b[^>]*>").expect("tags")
    });
    let text = HIDDEN.replace_all(text, "");
    let text = TAG.replace_all(&text, "");
    PICTURE.replace_all(&text, "[$1]").trim().to_owned()
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn author(value: &Value) -> String {
    value["login"]
        .as_str()
        .filter(|login| !login.is_empty())
        .unwrap_or("ghost")
        .to_owned()
}

fn checks(value: &Value) -> Vec<Check> {
    let mut checks: Vec<Check> = value
        .as_array()
        .into_iter()
        .flatten()
        .map(|check| {
            let url = check["detailsUrl"]
                .as_str()
                .or(check["targetUrl"].as_str())
                .filter(|url| url.starts_with("https://"))
                .map(str::to_owned);
            let state = match check["state"].as_str() {
                // A commit status, from a service.
                Some("SUCCESS") => CheckState::Passed,
                Some("FAILURE" | "ERROR") => CheckState::Failed,
                Some(_) => CheckState::Pending,
                None if check["status"] != "COMPLETED" => CheckState::Pending,
                None => match check["conclusion"].as_str() {
                    Some("SUCCESS") => CheckState::Passed,
                    Some("SKIPPED" | "NEUTRAL") => CheckState::Skipped,
                    _ => CheckState::Failed,
                },
            };
            let name = check["name"].as_str().or(check["context"].as_str());
            Check {
                name: name.unwrap_or_default().to_owned(),
                workflow: check["workflowName"]
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned),
                state,
                job: url
                    .as_deref()
                    .and_then(|url| url.split("/job/").nth(1))
                    .and_then(|job| job.split(['?', '#', '/']).next()?.parse().ok()),
                url,
            }
        })
        .collect();
    // What failed first, then what is not done.
    checks.sort_by_key(|check| match check.state {
        CheckState::Failed => 0,
        CheckState::Pending => 1,
        CheckState::Passed => 2,
        CheckState::Skipped => 3,
    });
    checks
}

/// How all the checks went.
pub(super) fn overall(checks: &[Check]) -> Option<CheckState> {
    let any = |state| checks.iter().any(|check| check.state == state);
    if checks.is_empty() {
        None
    } else if any(CheckState::Failed) {
        Some(CheckState::Failed)
    } else if any(CheckState::Pending) {
        Some(CheckState::Pending)
    } else {
        Some(CheckState::Passed)
    }
}

fn pull(value: &Value) -> Option<Pull> {
    Some(Pull {
        number: value["number"].as_u64()?,
        title: text(&value["title"]),
        author: author(&value["author"]),
        head: text(&value["headRefName"]),
        base: text(&value["baseRefName"]),
        draft: value["isDraft"].as_bool().unwrap_or(false),
        state: match value["state"].as_str() {
            Some("MERGED") => PullState::Merged,
            Some("CLOSED") => PullState::Closed,
            _ => PullState::Open,
        },
        updated: seconds(value["updatedAt"].as_str().unwrap_or_default()),
        decision: match value["reviewDecision"].as_str() {
            Some("APPROVED") => Decision::Approved,
            Some("CHANGES_REQUESTED") => Decision::Changes,
            Some("REVIEW_REQUIRED") => Decision::Required,
            _ => Decision::None,
        },
        checks: checks(&value["statusCheckRollup"]),
        url: text(&value["url"]),
    })
}

fn details(value: &Value) -> Details {
    let mut talk: Vec<Said> = Vec::new();
    for comment in value["comments"].as_array().into_iter().flatten() {
        talk.push(Said {
            author: author(&comment["author"]),
            body: readable(comment["body"].as_str().unwrap_or_default()),
            at: seconds(comment["createdAt"].as_str().unwrap_or_default()),
            review: None,
        });
    }
    for review in value["reviews"].as_array().into_iter().flatten() {
        let state = text(&review["state"]);
        let body = readable(review["body"].as_str().unwrap_or_default());
        // A review that only holds comments on lines says nothing here.
        if body.is_empty() && state == "COMMENTED" {
            continue;
        }
        talk.push(Said {
            author: author(&review["author"]),
            body,
            at: seconds(review["submittedAt"].as_str().unwrap_or_default()),
            review: Some(state),
        });
    }
    talk.sort_by_key(|said| said.at);
    Details {
        body: readable(value["body"].as_str().unwrap_or_default()),
        head_oid: text(&value["headRefOid"]),
        base_oid: text(&value["baseRefOid"]),
        mergeable: match value["mergeable"].as_str() {
            Some("MERGEABLE") => Some(true),
            Some("CONFLICTING") => Some(false),
            _ => None,
        },
        additions: value["additions"].as_u64().unwrap_or_default(),
        deletions: value["deletions"].as_u64().unwrap_or_default(),
        files: value["files"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|file| PullFile {
                path: text(&file["path"]),
                additions: file["additions"].as_u64().unwrap_or_default(),
                deletions: file["deletions"].as_u64().unwrap_or_default(),
            })
            .collect(),
        talk,
    }
}

/// The comments on lines, which come as one list or as several pages one
/// after the other.
fn comments(output: &str) -> Vec<Comment> {
    serde_json::Deserializer::from_str(output)
        .into_iter::<Value>()
        .map_while(Result::ok)
        .flat_map(|page| match page {
            Value::Array(comments) => comments,
            other => vec![other],
        })
        .filter_map(|comment| {
            Some(Comment {
                id: comment["id"].as_u64()?,
                path: text(&comment["path"]),
                line: comment["line"]
                    .as_u64()
                    .and_then(|line| u32::try_from(line).ok()),
                left: comment["side"] == "LEFT",
                author: author(&comment["user"]),
                body: readable(comment["body"].as_str().unwrap_or_default()),
                at: seconds(comment["created_at"].as_str().unwrap_or_default()),
                reply_to: comment["in_reply_to_id"].as_u64(),
            })
        })
        .collect()
}

/// The comments by line, each with its replies, in their order.
fn threads(comments: Vec<Comment>) -> Vec<Thread> {
    let mut order: Vec<u64> = Vec::new();
    let mut by_first: HashMap<u64, Vec<Comment>> = HashMap::new();
    for comment in comments {
        let first = comment.reply_to.unwrap_or(comment.id);
        if !by_first.contains_key(&first) {
            order.push(first);
        }
        by_first.entry(first).or_default().push(comment);
    }
    order
        .into_iter()
        .filter_map(|first| {
            let mut comments = by_first.remove(&first)?;
            comments.sort_by_key(|comment| comment.at);
            let head = comments.iter().find(|comment| comment.id == first);
            let head = head.or(comments.first())?.clone();
            Some(Thread {
                path: head.path,
                line: head.line,
                left: head.left,
                comments,
            })
        })
        .collect()
}

/// `gh`, in a project.
#[derive(Clone, Debug)]
pub(super) struct Gh {
    program: PathBuf,
    cwd: PathBuf,
}

/// In tests, the `gh` to run.
#[cfg(test)]
pub(super) static FAKE_GH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

impl Gh {
    /// Slow the first time: run it away from the interface.
    pub(super) fn find(cwd: &Path) -> Option<Self> {
        #[cfg(test)]
        {
            let program = FAKE_GH
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()?;
            Some(Self {
                program,
                cwd: cwd.to_path_buf(),
            })
        }
        #[cfg(not(test))]
        Some(Self {
            program: lsp::servers::which_in("gh", lsp::servers::login_environment())?,
            cwd: cwd.to_path_buf(),
        })
    }

    fn run(&self, args: &[&str], input: Option<&str>) -> Result<String, String> {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .current_dir(&self.cwd)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(not(test))]
        command.envs(lsp::servers::login_environment());
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        if let Some(mut stdin) = child.stdin.take()
            && let Some(input) = input
        {
            let _ = stdin.write_all(input.as_bytes());
        }
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let said = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        // An answer that is an error has its reason in the body.
        let body: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
        let reason = body["errors"][0]["message"]
            .as_str()
            .or(body["message"].as_str());
        Err(match reason {
            Some(reason) if !said.contains(reason) => format!("{said}\n{reason}"),
            _ if said.is_empty() => "gh".to_owned(),
            _ => said,
        })
    }

    fn json(&self, args: &[&str]) -> Result<Value, String> {
        let output = self.run(args, None)?;
        serde_json::from_str(&output).map_err(|error| error.to_string())
    }

    pub(super) fn list(&self, filter: Filter) -> Result<Vec<Pull>, String> {
        let mut args = vec!["pr", "list", "--limit", "60", "--json", LIST_FIELDS];
        args.extend(filter.args());
        let list = self.json(&args)?;
        Ok(list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(pull)
            .collect())
    }

    /// A pull request, or the branch's own with `None`; `Ok(None)` when the
    /// branch has none.
    pub(super) fn view(&self, number: Option<u64>) -> Result<Option<(Pull, Details)>, String> {
        let number = number.map(|number| number.to_string());
        let mut args = vec!["pr", "view"];
        args.extend(number.as_deref());
        args.extend(["--json", DETAIL_FIELDS]);
        match self.json(&args) {
            Ok(value) => Ok(pull(&value).map(|pull| (pull, details(&value)))),
            Err(error) if number.is_none() && error.contains("no pull requests found") => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(super) fn threads(&self, number: u64) -> Result<Vec<Thread>, String> {
        let path = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments?per_page=100");
        let output = self.run(&["api", "--paginate", &path], None)?;
        Ok(threads(comments(&output)))
    }

    pub(super) fn checkout(&self, number: u64) -> Result<String, String> {
        self.run(&["pr", "checkout", &number.to_string()], None)
    }

    pub(super) fn merge(&self, number: u64, how: &str) -> Result<String, String> {
        self.run(&["pr", "merge", &number.to_string(), how], None)
    }

    pub(super) fn review(
        &self,
        number: u64,
        verdict: Verdict,
        body: &str,
    ) -> Result<String, String> {
        let flag = match verdict {
            Verdict::Approve => "--approve",
            Verdict::RequestChanges => "--request-changes",
            Verdict::Comment => "--comment",
        };
        let number = number.to_string();
        let mut args = vec!["pr", "review", &number, flag];
        if !body.is_empty() {
            args.extend(["--body-file", "-"]);
        }
        self.run(&args, Some(body))
    }

    pub(super) fn create(&self, title: &str, body: &str, draft: bool) -> Result<String, String> {
        let mut args = vec!["pr", "create", "--title", title, "--body-file", "-"];
        if draft {
            args.push("--draft");
        }
        self.run(&args, Some(body)).map(|url| url.trim().to_owned())
    }

    /// Answers a thread.
    pub(super) fn reply(&self, number: u64, first: u64, body: &str) -> Result<String, String> {
        let path = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments/{first}/replies");
        self.run(
            &["api", "--method", "POST", &path, "-F", "body=@-"],
            Some(body),
        )
    }

    /// A new comment on a line of the newer version.
    pub(super) fn comment(
        &self,
        number: u64,
        head: &str,
        path: &str,
        line: u32,
        body: &str,
    ) -> Result<String, String> {
        let api = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments");
        let (head, path, line) = (
            format!("commit_id={head}"),
            format!("path={path}"),
            format!("line={line}"),
        );
        self.run(
            &[
                "api",
                "--method",
                "POST",
                &api,
                "-F",
                "body=@-",
                "-f",
                &head,
                "-f",
                &path,
                "-F",
                &line,
                "-f",
                "side=RIGHT",
            ],
            Some(body),
        )
    }

    /// The end of the log of a job that failed.
    pub(super) fn failed_log(&self, job: u64) -> Result<String, String> {
        const LINES: usize = 150;
        let log = self.run(
            &["run", "view", "--job", &job.to_string(), "--log-failed"],
            None,
        )?;
        let lines: Vec<&str> = log.lines().collect();
        let start = lines.len().saturating_sub(LINES);
        Ok(lines[start..].join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn times_and_texts_for_reading() {
        assert_eq!(seconds("1970-01-01T00:00:00Z"), 0);
        assert_eq!(seconds("2026-11-19T18:08:12Z"), 1_795_111_692);
        assert_eq!(seconds("2024-02-29T12:00:00Z"), 1_709_208_000);
        assert_eq!(seconds("chissà"), 0);
        assert_eq!(
            readable("<!-- nascosto\n -->Ciao ![foto](https://x/y.png) <img src='https://x'/>fine"),
            "Ciao [foto](https://x/y.png) fine"
        );
    }

    #[test]
    fn a_pull_request_and_its_checks() {
        let value = json!({
            "number": 12, "title": "Titolo", "author": { "login": "ada" },
            "headRefName": "feature", "baseRefName": "main", "isDraft": true, "state": "OPEN",
            "updatedAt": "2026-11-19T18:08:12Z", "reviewDecision": "CHANGES_REQUESTED", "url": "https://github.com/a/b/pull/12",
            "statusCheckRollup": [
                { "__typename": "CheckRun", "name": "lint", "status": "COMPLETED", "conclusion": "SUCCESS", "workflowName": "CI", "detailsUrl": "https://github.com/a/b/actions/runs/1/job/77" },
                { "__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://github.com/a/b/actions/runs/1/job/78?pr=12" },
                { "__typename": "CheckRun", "name": "lento", "status": "IN_PROGRESS", "conclusion": "" },
                { "__typename": "StatusContext", "context": "deploy", "state": "PENDING", "targetUrl": "javascript:alert(1)" },
                { "__typename": "CheckRun", "name": "salta", "status": "COMPLETED", "conclusion": "SKIPPED" },
            ],
            "body": "Cosa fa", "headRefOid": "abc", "baseRefOid": "def", "mergeable": "CONFLICTING",
            "additions": 5, "deletions": 2, "files": [{ "path": "a.rs", "additions": 5, "deletions": 2 }],
            "reviews": [
                { "author": { "login": "bo" }, "state": "COMMENTED", "body": "", "submittedAt": "2026-11-19T18:00:00Z" },
                { "author": { "login": "bo" }, "state": "APPROVED", "body": "Bene", "submittedAt": "2026-11-19T18:01:00Z" },
            ],
            "comments": [{ "author": { "login": "" }, "body": "Prima", "createdAt": "2026-11-19T17:00:00Z" }],
        });
        let read = pull(&value).unwrap();
        assert_eq!(
            (read.number, read.draft, read.decision),
            (12, true, Decision::Changes)
        );
        let states: Vec<_> = read
            .checks
            .iter()
            .map(|check| (check.name.as_str(), check.state))
            .collect();
        assert_eq!(
            states,
            [
                ("test", CheckState::Failed),
                ("lento", CheckState::Pending),
                ("deploy", CheckState::Pending),
                ("lint", CheckState::Passed),
                ("salta", CheckState::Skipped),
            ]
        );
        assert_eq!(read.checks[0].job, Some(78));
        // Only web addresses are opened.
        assert_eq!(read.checks[2].url, None);
        assert_eq!(overall(&read.checks), Some(CheckState::Failed));
        assert_eq!(overall(&[]), None);
        let details = details(&value);
        assert_eq!(details.mergeable, Some(false));
        let talk: Vec<_> = details
            .talk
            .iter()
            .map(|said| (said.author.as_str(), said.body.as_str()))
            .collect();
        assert_eq!(talk, [("ghost", "Prima"), ("bo", "Bene")]);
    }

    #[test]
    fn comments_make_threads_across_pages() {
        let comment = |id: u64, line: Value, reply: Value, at: &str| json!({ "id": id, "path": "a.rs", "line": line, "side": "RIGHT", "user": { "login": "ada" }, "body": format!("n. {id}"), "created_at": at, "in_reply_to_id": reply });
        let first = json!([
            comment(1, json!(7), Value::Null, "2026-01-01T10:00:00Z"),
            comment(2, Value::Null, Value::Null, "2026-01-01T11:00:00Z"),
        ]);
        let second = json!([comment(3, json!(7), json!(1), "2026-01-02T10:00:00Z")]);
        let threads = threads(comments(&format!("{first}{second}")));
        assert_eq!(threads.len(), 2);
        assert_eq!((threads[0].line, threads[0].comments.len()), (Some(7), 2));
        assert_eq!(threads[0].comments[1].body, "n. 3");
        assert_eq!(threads[1].line, None);
    }
}
