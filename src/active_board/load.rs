pub(crate) trait ActiveBoardLoadHost {
    fn clear_board_local_transients(&mut self);
    fn clear_canvas_for_active_board_load(&mut self);
    fn load_active_board_viewport(&mut self);
    fn enqueue_active_board_existing_items(&mut self);
}

pub(crate) fn load_active_board(host: &mut impl ActiveBoardLoadHost) {
    host.clear_board_local_transients();
    host.clear_canvas_for_active_board_load();
    host.load_active_board_viewport();
    host.enqueue_active_board_existing_items();
}

#[cfg(test)]
mod tests {
    use super::{ActiveBoardLoadHost, load_active_board};

    #[derive(Default)]
    struct FakeHost {
        calls: Vec<&'static str>,
    }

    impl ActiveBoardLoadHost for FakeHost {
        fn clear_board_local_transients(&mut self) {
            self.calls.push("clear_transients");
        }

        fn clear_canvas_for_active_board_load(&mut self) {
            self.calls.push("clear_canvas");
        }

        fn load_active_board_viewport(&mut self) {
            self.calls.push("load_viewport");
        }

        fn enqueue_active_board_existing_items(&mut self) {
            self.calls.push("enqueue_existing");
        }
    }

    #[test]
    fn active_board_load_runs_runtime_steps_in_order() {
        let mut host = FakeHost::default();

        load_active_board(&mut host);

        assert_eq!(
            host.calls,
            vec![
                "clear_transients",
                "clear_canvas",
                "load_viewport",
                "enqueue_existing"
            ]
        );
    }
}
