//! The supply-chain scan for a skill an operator did not write.
//!
//! A skill's document, its catalogue metadata and each of its bundled files all
//! reach an agent's context verbatim. For a registry install or a console
//! upload that text was authored by someone other than the operator, so it is
//! untrusted input to a prompt — the same class as a fetched page or an MCP
//! tool description.
//!
//! [`scan_skill`](crate::scan_skill) inspects **all** of it, not just the body. The published
//! failures are the reason: a scanner that reads only `SKILL.md` is defeated by
//! a poisoned description, which lands in the prompt catalogue without ever
//! being looked at.
//!
//! ## Verdicts
//!
//! `pass`, `warn`, `block`. Warn is the default: a finding proceeds and the
//! operator sees it. Three things block — invisible, bidirectional and
//! zero-width code points; hard-coded credentials; and a bundled file whose
//! path escapes the skill's own directory — because none has a legitimate
//! reading in a skill and each is cheap to produce. An override is a
//! per-request force flag on the one install, never a setting that silences a
//! class of finding for a whole host.
//!
//! ## What this is not
//!
//! A static scan is bypassable — a pattern check loses to a string the skill
//! constructs at run time. This raises the cost of the cheap attacks and leaves
//! an audit record; the containment that actually holds is the tool-call gate.
//! Nothing here should be described to an operator as a sandbox.

mod checks;

pub use checks::is_invisible;

/// How severely a finding is treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// Nothing found.
    Pass,
    /// Found something an operator should see; the write proceeds.
    Warn,
    /// Found something that must not be stored without an explicit override.
    Block,
}

/// Which check produced a finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanCheck {
    /// Invisible, bidirectional or zero-width code points — text that renders
    /// as one thing to a reviewer and reads as another to a model.
    InvisibleCodePoints,
    /// A credential literal committed into the document.
    HardcodedCredential,
    /// Text addressed to the agent, in a field that should describe a
    /// procedure rather than instruct the reader.
    InstructionShaped,
    /// A shell pipeline that fetches and executes, or reads a credential path.
    ShellExfiltration,
    /// A reference to an MCP tool the skill does not declare.
    McpReference,
    /// A bundled file whose name is an archive, an executable, or an escape
    /// from the skill's own directory.
    ResourceShape,
}

/// Which piece of a skill a finding came from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanField {
    /// The frontmatter `name`.
    Name,
    /// The frontmatter `description`.
    Description,
    /// The frontmatter `category`.
    Category,
    /// The frontmatter `version`.
    Version,
    /// The Markdown body.
    Body,
    /// A frontmatter line this parser does not recognise, verbatim.
    ///
    /// An uploaded document is stored as its own source, so an unknown key
    /// reaches the agent exactly as written even though nothing reads it as a
    /// field. Scanned under its own name so a finding says where it came from.
    Frontmatter(String),
    /// A bundled file, by its path within the skill directory.
    Resource(String),
}

impl ScanField {
    /// The field in operator-facing language.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Name => "name".to_string(),
            Self::Description => "description".to_string(),
            Self::Category => "category".to_string(),
            Self::Version => "version".to_string(),
            Self::Body => "the document body".to_string(),
            Self::Frontmatter(line) => {
                let key = line
                    .split_once(':')
                    .map_or("<unrecognised>", |(key, _)| key.trim());
                let key: String = key
                    .chars()
                    .filter(|c| !checks::is_invisible(*c))
                    .take(40)
                    .collect();
                format!("the frontmatter line `{key}`")
            }
            Self::Resource(path) => {
                let path: String = path
                    .chars()
                    .filter(|c| !checks::is_invisible(*c))
                    .take(40)
                    .collect();
                format!("the bundled file `{}`", path.replace('`', "’"))
            }
        }
    }
}

/// One thing the scan found.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Which check fired.
    pub check: ScanCheck,
    /// How severely it is treated.
    pub verdict: Verdict,
    /// Where in the skill it fired.
    pub field: ScanField,
    /// What was found, in operator-facing language and without echoing the
    /// offending value.
    pub detail: String,
}

impl Finding {
    /// The finding as one line an operator can read.
    #[must_use]
    pub fn message(&self) -> String {
        format!("{} in {}", self.detail, self.field.label())
    }
}

/// Everything one scan found.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    /// Every finding, in the order the fields were inspected.
    pub findings: Vec<Finding>,
}

impl ScanReport {
    /// The report's overall verdict: the worst of its findings, `Pass` when
    /// there are none.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        self.findings
            .iter()
            .map(|finding| finding.verdict)
            .max()
            .unwrap_or(Verdict::Pass)
    }

    /// Whether this report refuses the write absent an explicit override.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.verdict() == Verdict::Block
    }

    /// One readable line per finding.
    #[must_use]
    pub fn messages(&self) -> Vec<String> {
        self.findings.iter().map(Finding::message).collect()
    }
}

/// A file bundled alongside a skill's `SKILL.md`.
///
/// `path` is relative to the skill's own directory; `text` is the file's
/// content, lossily decoded — a binary file still gets its name checked.
#[derive(Clone, Debug)]
pub struct ScanResource {
    /// The file's path within the skill directory.
    pub path: String,
    /// The file's content.
    pub text: String,
}

/// The text surfaces of one skill document, borrowed from however the host
/// keeps it.
///
/// `extra_frontmatter` is every frontmatter line the host's parser did not
/// recognise, verbatim: a stored document reaches the agent as written, so an
/// unknown key is as agent-visible as a known one.
#[derive(Clone, Copy, Debug)]
pub struct ScanDocument<'a> {
    /// The frontmatter `name`.
    pub name: &'a str,
    /// The frontmatter `description`.
    pub description: &'a str,
    /// The frontmatter `category`, when there is one.
    pub category: Option<&'a str>,
    /// The frontmatter `version`, when there is one.
    pub version: Option<&'a str>,
    /// The Markdown body.
    pub body: &'a str,
    /// Unrecognised frontmatter lines, verbatim.
    pub extra_frontmatter: &'a [String],
}

/// Scans a skill document and its bundled resources.
///
/// Every text surface that can reach an agent is inspected: the frontmatter
/// scalars a prompt catalogue interpolates, the body the read tools return,
/// any unrecognised frontmatter line, and each bundled file's name and
/// content.
#[must_use]
pub fn scan_skill(doc: &ScanDocument<'_>, resources: &[ScanResource]) -> ScanReport {
    let mut findings = Vec::new();

    let mut fields = vec![
        (ScanField::Name, doc.name),
        (ScanField::Description, doc.description),
    ];
    if let Some(category) = doc.category {
        fields.push((ScanField::Category, category));
    }
    if let Some(version) = doc.version {
        fields.push((ScanField::Version, version));
    }
    fields.push((ScanField::Body, doc.body));
    for line in doc.extra_frontmatter {
        fields.push((ScanField::Frontmatter(line.clone()), line.as_str()));
    }

    for (field, text) in fields {
        checks::scan_text(&field, text, &mut findings);
    }

    for resource in resources {
        let field = ScanField::Resource(resource.path.clone());
        if let Some((verdict, detail)) = checks::resource_path_problem(&resource.path) {
            findings.push(Finding {
                check: ScanCheck::ResourceShape,
                verdict,
                field: field.clone(),
                detail,
            });
        }
        // The path is as agent-visible as the content — a bundled file's name
        // is what a read tool reports — so it gets the same text checks.
        checks::scan_text(&field, &resource.path, &mut findings);
        checks::scan_text(&field, &resource.text, &mut findings);
    }

    ScanReport { findings }
}

/// Renders untrusted text so it can sit inside a prompt as data.
///
/// Strips the code points [`is_invisible`] names, folds every run of
/// whitespace to one space so a value cannot introduce a line — or a turn
/// boundary — of its own, turns backticks into quotes so it cannot open a code
/// fence, escapes the quote and angle-bracket characters the surrounding
/// template uses as structure, and caps the length.
///
/// This closes the poisoned-description shape structurally rather than by
/// detection, which is why it runs on every skill regardless of verdict: a
/// description that passed the scan is still text somebody else wrote.
#[must_use]
pub fn sanitize_catalogue_text(text: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if checks::is_invisible(c) {
            continue;
        }
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        match c {
            '"' => out.push_str("&quot;"),
            '\\' => out.push_str("&#92;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '`' => out.push('\''),
            _ => out.push(c),
        }
    }
    if out.chars().count() > max_chars {
        out = out.chars().take(max_chars).collect::<String>() + "…";
    }
    out
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
