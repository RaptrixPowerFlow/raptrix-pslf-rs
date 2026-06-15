// raptrix-pslf-rs — deterministic equipment mrid synthesis (vendor export path).

use std::collections::HashMap;

use crate::models::Transformer3W;

pub const SYNTHETIC_STAR_BUS_MIN_ID_EXCLUSIVE: u32 = 10_000_000;

pub fn synth_branch_mrid(from_bus: u32, to_bus: u32, ckt: &str) -> String {
    format!("BR_{from_bus}_{to_bus}_{ckt}")
}

pub fn synth_generator_mrid(bus_id: u32, machine_id: &str) -> String {
    format!("GEN_{bus_id}_{machine_id}")
}

pub fn synth_transformer_2w_mrid(from_bus: u32, to_bus: u32, ckt: &str) -> String {
    format!("XF2_{from_bus}_{to_bus}_{ckt}")
}

pub fn synth_transformer_3w_mrid(bus_h: u32, bus_m: u32, bus_l: u32, ckt: &str) -> String {
    format!("XF3_{bus_h}_{bus_m}_{bus_l}_{ckt}")
}

/// Maps `(star_bus_id, endpoint_bus_id)` → `{parent_mrid}_{H|M|L}` for expanded 3W legs.
pub fn build_star_leg_mrid_map(transformers_3w: &[Transformer3W]) -> HashMap<(u32, u32), String> {
    let mut map = HashMap::new();
    for tx3 in transformers_3w {
        if tx3.status == 0 {
            continue;
        }
        let parent = synth_transformer_3w_mrid(tx3.bus_h, tx3.bus_m, tx3.bus_l, tx3.ckt.as_ref());
        let star = tx3.star_bus_id;
        map.insert((star, tx3.bus_h), format!("{parent}_H"));
        map.insert((star, tx3.bus_m), format!("{parent}_M"));
        map.insert((star, tx3.bus_l), format!("{parent}_L"));
    }
    map
}

pub fn synth_transformer_2w_mrid_with_star_legs(
    from_bus: u32,
    to_bus: u32,
    ckt: &str,
    star_leg_map: &HashMap<(u32, u32), String>,
) -> String {
    if from_bus > SYNTHETIC_STAR_BUS_MIN_ID_EXCLUSIVE {
        if let Some(mrid) = star_leg_map.get(&(from_bus, to_bus)) {
            return mrid.clone();
        }
    }
    if to_bus > SYNTHETIC_STAR_BUS_MIN_ID_EXCLUSIVE {
        if let Some(mrid) = star_leg_map.get(&(to_bus, from_bus)) {
            return mrid.clone();
        }
    }
    synth_transformer_2w_mrid(from_bus, to_bus, ckt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_rules_match_contract() {
        assert_eq!(synth_branch_mrid(1, 2, "1"), "BR_1_2_1");
        assert_eq!(synth_generator_mrid(100, "G1"), "GEN_100_G1");
        assert_eq!(synth_transformer_2w_mrid(10, 20, "2"), "XF2_10_20_2");
        assert_eq!(synth_transformer_3w_mrid(1, 2, 3, "1"), "XF3_1_2_3_1");
    }
}
