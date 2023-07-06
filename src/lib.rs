use std::{collections::HashMap, fmt, str::FromStr};

use anyhow::*;
use cln_rpc::{
    model::ListpeerchannelsChannels,
    primitives::{Amount, ShortChannelId},
};
use log::debug;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq)]
pub enum SatDirection {
    #[serde(alias = "pull")]
    Pull,
    #[serde(alias = "push")]
    Push,
}

impl FromStr for SatDirection {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pull" => Ok(SatDirection::Pull),
            "push" => Ok(SatDirection::Push),
            _ => Err(anyhow!("could not parse flow direction from `{}`", s)),
        }
    }
}
impl fmt::Display for SatDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            SatDirection::Pull => write!(f, "pull"),
            SatDirection::Push => write!(f, "push"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Job {
    pub sat_direction: SatDirection,
    pub amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outppm: Option<u64>,
    pub maxppm: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidatelist: Option<Vec<ShortChannelId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxhops: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depleteuptopercent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depleteuptoamount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paralleljobs: Option<u8>,
}

impl Job {
    pub fn is_balanced(
        &self,
        channel: &ListpeerchannelsChannels,
        chan_id: &ShortChannelId,
    ) -> bool {
        let target_cap = self.target_cap(channel);
        debug!(
            "{}: target: {}sats",
            chan_id.to_string(),
            target_cap / 1_000
        );

        let channel_msat = Amount::msat(&channel.total_msat.unwrap());
        let to_us_msat = Amount::msat(&channel.to_us_msat.unwrap());

        match self.sat_direction {
            SatDirection::Pull => to_us_msat >= target_cap,
            SatDirection::Push => channel_msat - to_us_msat >= target_cap,
        }
    }
    pub fn target_cap(&self, channel: &ListpeerchannelsChannels) -> u64 {
        let target = self.target.unwrap_or(0.5);

        let total_msat = Amount::msat(&channel.total_msat.unwrap());
        let their_reserve_msat = Amount::msat(&channel.their_reserve_msat.unwrap());
        let our_reserve_msat = Amount::msat(&channel.our_reserve_msat.unwrap());

        let mut target_cap = (total_msat as f64 * target) as u64;
        match self.sat_direction {
            SatDirection::Pull => {
                if target_cap >= total_msat - their_reserve_msat - 1_000 {
                    target_cap = total_msat - their_reserve_msat - 2_000;
                }
            }
            SatDirection::Push => {
                if target_cap >= total_msat - our_reserve_msat - 1_000 {
                    target_cap = total_msat - our_reserve_msat - 2_000;
                }
            }
        }
        target_cap
    }
    pub fn to_json(&self) -> serde_json::Value {
        let mut result = HashMap::new();
        result.insert("direction", self.sat_direction.to_string());
        result.insert("amount", (self.amount / 1_000).to_string());
        result.insert("maxppm", self.maxppm.to_string());
        match self.outppm {
            Some(o) => result.insert("outppm", o.to_string()),
            None => None,
        };
        match self.target {
            Some(t) => result.insert("target", t.to_string()),
            None => None,
        };
        match self.maxhops {
            Some(m) => result.insert("maxhops", m.to_string()),
            None => None,
        };
        match &self.candidatelist {
            Some(c) => result.insert(
                "candidates",
                c.iter()
                    .map(|y| y.to_string())
                    .collect::<Vec<String>>()
                    .join(", "),
            ),
            None => None,
        };
        match self.depleteuptopercent {
            Some(dp) => result.insert("depleteuptopercent", dp.to_string()),
            None => None,
        };
        match self.depleteuptoamount {
            Some(da) => result.insert("depleteuptoamount", (da / 1_000).to_string()),
            None => None,
        };
        match self.paralleljobs {
            Some(pj) => result.insert("paralleljobs", pj.to_string()),
            None => None,
        };
        json!(result)
    }
}
