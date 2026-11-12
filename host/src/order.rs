//! The order of the events of a clip.

/// Joins the controller events and the note events of a clip into one
/// list sorted by frame.
///
/// At one frame the controllers come first, so a pitch bend at a note's
/// start applies to the note. Within each kind the order stays as given.
/// The rule rests on the sort being stable, which the unit tests guard.
pub(crate) fn clip_order<E>(
    controls: Vec<(usize, E)>,
    notes: impl IntoIterator<Item = (usize, E)>,
) -> Vec<(usize, E)> {
    let mut events = controls;
    events.extend(notes);
    events.sort_by_key(|(frame, _)| *frame);
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_come_before_notes_at_the_same_frame() {
        let controls = vec![(10, "bend a"), (0, "wheel"), (10, "bend b"), (20, "bend c")];
        let notes = vec![(0, "on 1"), (10, "on 2"), (10, "on 3"), (15, "off 1")];
        assert_eq!(
            clip_order(controls, notes),
            vec![
                (0, "wheel"),
                (0, "on 1"),
                (10, "bend a"),
                (10, "bend b"),
                (10, "on 2"),
                (10, "on 3"),
                (15, "off 1"),
                (20, "bend c"),
            ]
        );
    }

    /// The standard library sorts a slice of up to 20 items by insertion,
    /// which is stable even through `sort_unstable_by_key`. Sixty events on
    /// three frames take the general sort, which an unstable sort reorders.
    #[test]
    fn a_long_clip_keeps_the_given_order_within_a_frame() {
        let controls: Vec<(usize, String)> =
            (0..30).map(|i| (i % 3, format!("control {i}"))).collect();
        let notes: Vec<(usize, String)> = (0..30).map(|i| (i % 3, format!("note {i}"))).collect();
        let expected: Vec<(usize, String)> = (0..3)
            .flat_map(|frame| {
                let at = |events: &[(usize, String)]| {
                    events
                        .iter()
                        .filter(|(at, _)| *at == frame)
                        .cloned()
                        .collect::<Vec<_>>()
                };
                at(&controls).into_iter().chain(at(&notes))
            })
            .collect();
        assert_eq!(clip_order(controls, notes), expected);
    }
}
