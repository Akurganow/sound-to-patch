---
description: A note-rule lookup in the music dictionary.
expected_outcome: Kick at slots 0 4 8 12, bass at the twelve other slots, notes 0.125 to 0.19 beat long.
tags: [baseline]
max_turns: 10
allowed_tools: [Read, Glob, Grep, Skill]
---

I want sound-to-patch to write the notes of a rolling bass, in the default sense the sound-to-patch music dictionary gives the word. On a one-bar grid of 16 slots numbered 0 to 15, which slots hold the kick and which hold bass notes, and how long is each bass note in beats, as the dictionary states it?
