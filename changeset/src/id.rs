// TODO(ahuszagh) This needs custom parsers.
//

// the type for the identifier: if it's a PR,
// issue, commit hash, or other. this ID can
// be sorted to generate the changelog, or use
// the default sort order.
// TODO(ahuszagh) Hash should sort by the value
//  in the git history
//      AKA, git rev-hash
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identifier {
    PullRequest(Vec<u64>),
    Issue(Vec<u64>),
    Commit(Vec<u64>),
    Other(Vec<String>),
}

impl Identifier {
    fn is_pull_request(&self) -> bool {
        matches!(self, Identifier::PullRequest(_))
    }

    fn is_issue(&self) -> bool {
        matches!(self, Identifier::Issue(_))
    }

    fn is_commit(&self) -> bool {
        matches!(self, Identifier::Commit(_))
    }

    fn is_other(&self) -> bool {
        matches!(self, Identifier::Other(_))
    }
}

// sort
// by the number, otherwise, sort as 0. the numbers
// should be sorted, and the `max(values) || 0` should
// be used.
// TODO(ahuszagh) Nee

// TODO(ahuszagh) Should have a validator or parser type

// TODO(ahuszagh) Restore...
//impl IdType {
//    fn numbers(&self) -> &[u64] {
//        match self {
//            IdType::PullRequest(v) => v,
//            IdType::Issue(v) => v,
//            Id::Other => todo!(),
//        }
//    }
//
//    fn max_number(&self) -> u64 {
//        self.numbers().iter().max().map_or_else(|| 0, |v| *v)
//    }
//
//    // TODO(ahuszagh) Probably need a commit-based formatter.
//
//    fn parse_stem(file_stem: &str) -> cross::Result<IdType> {
//        let (is_issue, rest) = match file_stem.strip_prefix("issue") {
//            Some(n) => (true, n),
//            None => (false, file_stem),
//        };
//        let mut numbers = rest
//            .split('-')
//            .map(|x| x.parse::<u64>())
//            .collect::<Result<Vec<u64>, _>>()?;
//        numbers.sort_unstable();
//
//        Ok(match is_issue {
//            false => IdType::PullRequest(numbers),
//            true => IdType::Issue(numbers),
//        })
//    }
//
//    fn parse_changelog(prs: &str) -> cross::Result<IdType> {
//        let mut numbers = prs
//            .split(',')
//            .map(|x| x.trim().parse::<u64>())
//            .collect::<Result<Vec<u64>, _>>()?;
//        numbers.sort_unstable();
//
//        Ok(IdType::PullRequest(numbers))
//    }
//}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name() {
        unimplemented!();
    }
}
