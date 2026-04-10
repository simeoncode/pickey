use crate::config::Rule;
use glob_match::glob_match;

/// Match result from evaluating rules against an SSH invocation.
#[derive(Debug)]
pub struct MatchResult<'a> {
    pub rule: &'a Rule,
    pub rule_index: usize,
}

/// Find the first matching rule for the given host and path.
/// Rules are evaluated top-to-bottom, first match wins.
pub fn find_match<'a>(rules: &'a [Rule], host: &str, path: &str) -> Option<MatchResult<'a>> {
    for (i, rule) in rules.iter().enumerate() {
        if rule.host != host {
            continue;
        }

        match &rule.match_pattern {
            Some(pattern) => {
                if glob_match(pattern, path) {
                    return Some(MatchResult {
                        rule,
                        rule_index: i,
                    });
                }
            }
            // No match pattern = matches any path on this host
            None => {
                return Some(MatchResult {
                    rule,
                    rule_index: i,
                });
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Rule;

    fn rule(host: &str, match_pattern: Option<&str>, key: &str) -> Rule {
        Rule {
            host: host.to_string(),
            match_pattern: match_pattern.map(String::from),
            key: key.to_string(),
            email: None,
            name: None,
            port: None,
            auto: false,
        }
    }

    #[test]
    fn exact_org_match() {
        let rules = vec![
            rule(
                "github.com",
                Some("VolvoGroup-Internal/*"),
                "~/.ssh/id_volvo",
            ),
            rule("github.com", Some("Personal/*"), "~/.ssh/id_personal"),
        ];
        let m = find_match(&rules, "github.com", "VolvoGroup-Internal/repo").unwrap();
        assert_eq!(m.rule_index, 0);
        assert_eq!(m.rule.key, "~/.ssh/id_volvo");
    }

    #[test]
    fn second_rule_matches() {
        let rules = vec![
            rule(
                "github.com",
                Some("VolvoGroup-Internal/*"),
                "~/.ssh/id_volvo",
            ),
            rule("github.com", Some("Personal/*"), "~/.ssh/id_personal"),
        ];
        let m = find_match(&rules, "github.com", "Personal/repo").unwrap();
        assert_eq!(m.rule_index, 1);
    }

    #[test]
    fn no_match_returns_none() {
        let rules = vec![rule(
            "github.com",
            Some("VolvoGroup-Internal/*"),
            "~/.ssh/id_volvo",
        )];
        let m = find_match(&rules, "github.com", "UnknownOrg/repo");
        assert!(m.is_none());
    }

    #[test]
    fn host_mismatch() {
        let rules = vec![rule("github.com", Some("Org/*"), "~/.ssh/id")];
        let m = find_match(&rules, "gitlab.com", "Org/repo");
        assert!(m.is_none());
    }

    #[test]
    fn catch_all_host_rule() {
        let rules = vec![rule("gitlab.selfhosted.com", None, "~/.ssh/id_client")];
        let m = find_match(&rules, "gitlab.selfhosted.com", "any/path/here").unwrap();
        assert_eq!(m.rule.key, "~/.ssh/id_client");
    }

    #[test]
    fn first_match_wins() {
        let rules = vec![
            rule("github.com", Some("Org/*"), "~/.ssh/key1"),
            rule("github.com", Some("Org/*"), "~/.ssh/key2"),
        ];
        let m = find_match(&rules, "github.com", "Org/repo").unwrap();
        assert_eq!(m.rule.key, "~/.ssh/key1");
    }

    #[test]
    fn azure_devops_deep_path() {
        let rules = vec![rule(
            "ssh.dev.azure.com",
            Some("v3/ClientX/**"),
            "~/.ssh/id_clientx",
        )];
        let m = find_match(&rules, "ssh.dev.azure.com", "v3/ClientX/Project/Repo").unwrap();
        assert_eq!(m.rule.key, "~/.ssh/id_clientx");
    }

    #[test]
    fn gitlab_subgroups() {
        let rules = vec![rule("gitlab.com", Some("group/subgroup/*"), "~/.ssh/id_gl")];
        let m = find_match(&rules, "gitlab.com", "group/subgroup/repo").unwrap();
        assert_eq!(m.rule.key, "~/.ssh/id_gl");
    }
}
