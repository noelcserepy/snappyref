#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoardSwitchResult {
    Unchanged,
    Switched { active_board_id: i64 },
}

pub(crate) trait BoardSwitchHost {
    fn active_board_id(&self) -> i64;

    fn persist_active_board_before_switch(&mut self);
    fn set_active_board_in_library(&mut self, board_id: i64) -> Result<(), String>;
    fn load_active_board(&mut self);
}

pub(crate) fn switch_to_board(
    host: &mut impl BoardSwitchHost,
    target_board_id: i64,
) -> Result<BoardSwitchResult, String> {
    if host.active_board_id() == target_board_id {
        return Ok(BoardSwitchResult::Unchanged);
    }

    host.persist_active_board_before_switch();
    host.set_active_board_in_library(target_board_id)?;
    host.load_active_board();

    Ok(BoardSwitchResult::Switched {
        active_board_id: host.active_board_id(),
    })
}

#[cfg(test)]
mod tests {
    use super::{BoardSwitchHost, BoardSwitchResult, switch_to_board};

    #[derive(Default)]
    struct FakeHost {
        active_board_id: i64,
        set_active_board_error: Option<String>,
        calls: Vec<&'static str>,
    }

    impl BoardSwitchHost for FakeHost {
        fn active_board_id(&self) -> i64 {
            self.active_board_id
        }

        fn persist_active_board_before_switch(&mut self) {
            self.calls.push("persist");
        }

        fn set_active_board_in_library(&mut self, board_id: i64) -> Result<(), String> {
            self.calls.push("set_active");
            if let Some(error) = self.set_active_board_error.take() {
                return Err(error);
            }
            self.active_board_id = board_id;
            Ok(())
        }

        fn load_active_board(&mut self) {
            self.calls.push("load_active_board");
        }
    }

    #[test]
    fn unchanged_switch_does_not_touch_host() {
        let mut host = FakeHost {
            active_board_id: 7,
            ..Default::default()
        };

        let result = switch_to_board(&mut host, 7).unwrap();

        assert_eq!(result, BoardSwitchResult::Unchanged);
        assert!(host.calls.is_empty());
    }

    #[test]
    fn switch_persists_activates_library_and_loads_active_board() {
        let mut host = FakeHost {
            active_board_id: 1,
            ..Default::default()
        };

        let result = switch_to_board(&mut host, 2).unwrap();

        assert_eq!(result, BoardSwitchResult::Switched { active_board_id: 2 });
        assert_eq!(
            host.calls,
            vec!["persist", "set_active", "load_active_board"]
        );
    }

    #[test]
    fn failed_storage_activation_stops_before_clearing_current_board() {
        let mut host = FakeHost {
            active_board_id: 1,
            set_active_board_error: Some("cannot activate board".to_owned()),
            ..Default::default()
        };

        let result = switch_to_board(&mut host, 2);

        assert_eq!(result, Err("cannot activate board".to_owned()));
        assert_eq!(host.active_board_id, 1);
        assert_eq!(host.calls, vec!["persist", "set_active"]);
    }
}
