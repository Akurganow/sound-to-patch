//! The mapper seam: every ready synth's mapper meets the contract.

use sound_to_patch::preset::mapper::{MapContext, mapper_for};
use sound_to_patch::preset::registry::registry_for;
use sound_to_patch::synth::capabilities;

use crate::support::{for_each_ready_synth, mapper_contract};

#[test]
fn every_ready_mapper_meets_the_contract() {
    for_each_ready_synth(|synth| {
        let mapper = mapper_for(synth).unwrap();
        assert_eq!(mapper.synth(), synth);
        let ctx = MapContext {
            registry: registry_for(synth).unwrap(),
            capabilities: capabilities(synth).unwrap(),
        };
        mapper_contract(mapper, &ctx).unwrap();
    });
}
