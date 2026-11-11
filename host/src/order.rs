//! The order of the events of a clip.

/// Joins the controller events and the note events of a clip into one
/// list sorted by frame.
///
/// At one frame the controllers come first, so a pitch bend at a note's
/// start applies to the note. Within each kind the order stays as given.
/// The rule rests on the sort being stable, so the unit test guards it.
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
}
