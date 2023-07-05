use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Display, Formatter},
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Error};
use cln_rpc::{
    model::{ListchannelsChannels, ListpeerchannelsChannels},
    primitives::{Amount, PublicKey, ShortChannelId},
};
use log::debug;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tabled::Tabled;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

pub const SUCCESSES_SUFFIX: &str = "_successes.json";
pub const FAILURES_SUFFIX: &str = "_failures.json";
pub const NO_ALIAS_SET: &str = "NO_ALIAS_SET";

pub const PLUGIN_NAME: &str = "sling";
pub const GRAPH_FILE_NAME: &str = "graph.json";
pub const JOB_FILE_NAME: &str = "jobs.json";
pub const EXCEPTS_CHANS_FILE_NAME: &str = "excepts.json";
pub const EXCEPTS_PEERS_FILE_NAME: &str = "excepts_peers.json";

#[derive(Clone)]
pub struct PluginState {
    pub config: Arc<Mutex<Config>>,
    pub peer_channels: Arc<tokio::sync::Mutex<BTreeMap<String, ListpeerchannelsChannels>>>,
    pub graph: Arc<tokio::sync::Mutex<LnGraph>>,
    pub pays: Arc<RwLock<HashMap<String, String>>>,
    pub alias_peer_map: Arc<Mutex<HashMap<PublicKey, String>>>,
    pub pull_jobs: Arc<Mutex<HashSet<String>>>,
    pub push_jobs: Arc<Mutex<HashSet<String>>>,
    pub excepts_chans: Arc<Mutex<Vec<ShortChannelId>>>,
    pub excepts_peers: Arc<Mutex<Vec<PublicKey>>>,
    pub tempbans: Arc<Mutex<HashMap<String, u64>>>,
    pub job_state: Arc<Mutex<HashMap<String, Vec<JobState>>>>,
}
impl PluginState {
    pub fn new() -> PluginState {
        PluginState {
            config: Arc::new(Mutex::new(Config::new())),
            peer_channels: Arc::new(tokio::sync::Mutex::new(BTreeMap::new())),
            graph: Arc::new(tokio::sync::Mutex::new(LnGraph::new())),
            pays: Arc::new(RwLock::new(HashMap::new())),
            alias_peer_map: Arc::new(Mutex::new(HashMap::new())),
            pull_jobs: Arc::new(Mutex::new(HashSet::new())),
            push_jobs: Arc::new(Mutex::new(HashSet::new())),
            excepts_chans: Arc::new(Mutex::new(Vec::new())),
            excepts_peers: Arc::new(Mutex::new(Vec::new())),
            tempbans: Arc::new(Mutex::new(HashMap::new())),
            job_state: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub pubkey: Option<PublicKey>,
    pub utf8: (String, bool),
    pub refresh_peers_interval: (String, u64),
    pub refresh_aliasmap_interval: (String, u64),
    pub refresh_graph_interval: (String, u64),
    pub reset_liquidity_interval: (String, u64),
    pub depleteuptopercent: (String, f64),
    pub depleteuptoamount: (String, u64),
    pub paralleljobs: (String, u8),
    pub timeoutpay: (String, u16),
    pub max_htlc_count: (String, u64),
    pub lightning_cli: (String, String),
    pub stats_delete_failures_age: (String, u64),
    pub stats_delete_failures_size: (String, u64),
    pub stats_delete_successes_age: (String, u64),
    pub stats_delete_successes_size: (String, u64),
    pub cltv_delta: (String, Option<u16>),
    pub channel_health: (String, bool),
}
impl Config {
    pub fn new() -> Config {
        Config {
            pubkey: None,
            utf8: (PLUGIN_NAME.to_string() + "-utf8", true),
            refresh_peers_interval: (PLUGIN_NAME.to_string() + "-refresh-peers-interval", 1),
            refresh_aliasmap_interval: (
                PLUGIN_NAME.to_string() + "-refresh-aliasmap-interval",
                3600,
            ),
            refresh_graph_interval: (PLUGIN_NAME.to_string() + "-refresh-graph-interval", 600),
            reset_liquidity_interval: (PLUGIN_NAME.to_string() + "-reset-liquidity-interval", 360),
            depleteuptopercent: (PLUGIN_NAME.to_string() + "-depleteuptopercent", 0.2),
            depleteuptoamount: (
                PLUGIN_NAME.to_string() + "-depleteuptoamount",
                2_000_000_000,
            ),
            paralleljobs: (PLUGIN_NAME.to_string() + "-paralleljobs", 1),
            timeoutpay: (PLUGIN_NAME.to_string() + "-timeoutpay", 120),
            max_htlc_count: (PLUGIN_NAME.to_string() + "-max-htlc-count", 5),
            lightning_cli: (
                PLUGIN_NAME.to_string() + "-lightning-cli",
                "lightning-cli".to_string(),
            ),
            stats_delete_failures_age: (PLUGIN_NAME.to_string() + "-stats-delete-failures-age", 30),
            stats_delete_failures_size: (
                PLUGIN_NAME.to_string() + "-stats-delete-failures-size",
                10_000,
            ),
            stats_delete_successes_age: (
                PLUGIN_NAME.to_string() + "-stats-delete-successes-age",
                30,
            ),
            stats_delete_successes_size: (
                PLUGIN_NAME.to_string() + "-stats-delete-successes-size",
                10_000,
            ),
            cltv_delta: ("cltv-delta".to_string(), None),
            channel_health: (PLUGIN_NAME.to_string() + "-channel-health", false),
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobState {
    latest_state: JobMessage,
    active: bool,
    should_stop: bool,
    id: u8,
}
impl JobState {
    pub fn new(latest_state: JobMessage, id: u8) -> Self {
        JobState {
            latest_state,
            active: true,
            should_stop: false,
            id,
        }
    }
    pub fn missing() -> Self {
        JobState {
            latest_state: JobMessage::NoJob,
            active: false,
            should_stop: false,
            id: 0,
        }
    }
    pub fn statechange(&mut self, latest_state: JobMessage) {
        self.latest_state = latest_state;
    }
    pub fn state(&self) -> JobMessage {
        self.latest_state
    }
    pub fn stop(&mut self) {
        self.should_stop = true;
    }
    pub fn should_stop(&self) -> bool {
        self.should_stop
    }
    pub fn is_active(&self) -> bool {
        self.active
    }
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }
    pub fn id(&self) -> u8 {
        self.id
    }
}

#[derive(Debug, Clone, Copy)]
pub enum JobMessage {
    Starting,
    Rebalancing,
    Balanced,
    NoCandidates,
    HTLCcapped,
    Disconnected,
    PeerNotFound,
    PeerNotReady,
    ChanNotNormal,
    GraphEmpty,
    ChanNotInGraph,
    NoRoute,
    TooExp,
    Stopping,
    Stopped,
    Error,
    NoJob,
}
impl Display for JobMessage {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            JobMessage::Starting => write!(f, "Starting"),
            JobMessage::Rebalancing => write!(f, "Rebalancing"),
            JobMessage::Balanced => write!(f, "Balanced"),
            JobMessage::NoCandidates => write!(f, "NoCandidates"),
            JobMessage::HTLCcapped => write!(f, "HTLCcapped"),
            JobMessage::Disconnected => write!(f, "Disconnected"),
            JobMessage::PeerNotFound => write!(f, "PeerNotFound"),
            JobMessage::PeerNotReady => write!(f, "PeerNotReady"),
            JobMessage::ChanNotNormal => write!(f, "ChanNotNormal"),
            JobMessage::GraphEmpty => write!(f, "GraphEmpty"),
            JobMessage::ChanNotInGraph => write!(f, "ChanNotInGraph"),
            JobMessage::NoRoute => write!(f, "NoRoutes"),
            JobMessage::TooExp => write!(f, "NoCheapRoute"),
            JobMessage::Stopping => write!(f, "Stopping"),
            JobMessage::Stopped => write!(f, "Stopped"),
            JobMessage::Error => write!(f, "Error"),
            JobMessage::NoJob => write!(f, "NoJob"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DijkstraNode {
    pub score: u64,
    pub channel: ListchannelsChannels,
    pub destination: PublicKey,
    pub hops: u64,
}
impl PartialEq for DijkstraNode {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
            && self.hops == other.hops
            && self.channel.source == other.channel.source
            && self.channel.destination == other.channel.destination
            && self.channel.short_channel_id.to_string()
                == other.channel.short_channel_id.to_string()
            && self.destination == other.destination
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectedChannel {
    pub channel: ListchannelsChannels,
    pub liquidity: u64,
    pub timestamp: u64,
}
impl DirectedChannel {
    pub fn new(channel: ListchannelsChannels) -> DirectedChannel {
        DirectedChannel {
            liquidity: Amount::msat(&channel.htlc_maximum_msat.unwrap_or(channel.amount_msat)) / 2,
            channel,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LnGraph {
    pub graph: BTreeMap<PublicKey, Vec<DirectedChannel>>,
}
impl LnGraph {
    pub fn new() -> Self {
        LnGraph {
            graph: BTreeMap::new(),
        }
    }
    pub fn update(&mut self, new_graph: LnGraph) {
        for (new_node, new_channels) in new_graph.graph.iter() {
            let old_channels = self.graph.entry(new_node.clone()).or_default();
            let new_short_channel_ids: HashSet<String> = new_channels
                .iter()
                .map(|c| c.channel.short_channel_id.to_string())
                .collect();
            old_channels.retain(|e| {
                new_short_channel_ids.contains(&e.channel.short_channel_id.to_string())
            });
            for new_channel in new_channels {
                let new_short_channel_id = &new_channel.channel.short_channel_id;
                let old_channel = old_channels.iter_mut().find(|e| {
                    e.channel.short_channel_id.to_string() == new_short_channel_id.to_string()
                });
                match old_channel {
                    Some(old_channel) => {
                        if (old_channel.channel.htlc_maximum_msat.is_some()
                            && new_channel.channel.htlc_maximum_msat.is_some()
                            && old_channel.channel.htlc_maximum_msat.unwrap()
                                != new_channel.channel.htlc_maximum_msat.unwrap())
                            || old_channel.channel.fee_per_millionth
                                != new_channel.channel.fee_per_millionth
                        {
                            old_channel.liquidity = new_channel.liquidity;
                            old_channel.timestamp = new_channel.timestamp;
                        }
                        old_channel.channel = new_channel.channel.clone();
                    }
                    None => {
                        old_channels.push(new_channel.clone());
                    }
                }
            }
        }

        let new_nodes: HashSet<&PublicKey> = new_graph.graph.keys().collect();
        self.graph.retain(|k, _| new_nodes.contains(k));
    }
    pub fn refresh_liquidity(&mut self, interval: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut count = 0;
        for (_node, channels) in self.graph.iter_mut() {
            for channel in channels {
                if channel.timestamp <= now - interval * 60 {
                    channel.liquidity = Amount::msat(
                        &channel
                            .channel
                            .htlc_maximum_msat
                            .unwrap_or(channel.channel.amount_msat),
                    ) / 2;
                    channel.timestamp = now;
                    count += 1;
                }
            }
        }
        debug!("Reset liquidity belief on {} channels!", count);
    }
    pub fn get_channel(
        &self,
        source: &PublicKey,
        channel: &ShortChannelId,
    ) -> Result<DirectedChannel, Error> {
        match self.graph.get(source) {
            Some(e) => {
                let result = e
                    .into_iter()
                    .filter(|&i| i.channel.short_channel_id.to_string() == channel.to_string())
                    .collect::<Vec<&DirectedChannel>>();
                if result.len() != 1 {
                    Err(anyhow!(
                        "channel {} not found in graph",
                        channel.to_string()
                    ))
                } else {
                    Ok(result[0].clone())
                }
            }
            None => Err(anyhow!(
                "could not find channel in cached graph: {}",
                channel.to_string()
            )),
        }
    }

    pub fn edges(
        &self,
        mypubkey: &PublicKey,
        node: &PublicKey,
        exclude: &HashSet<String>,
        exclude_peers: &Vec<PublicKey>,
        amount: &u64,
        candidatelist: &Vec<ShortChannelId>,
        tempbans: &HashMap<String, u64>,
    ) -> Vec<&DirectedChannel> {
        match self.graph.get(&node) {
            Some(e) => {
                return e
                    .into_iter()
                    .filter(|&i| {
                        // debug!(
                        //     "{}: liq:{} amt:{}",
                        //     i.channel.short_channel_id.to_string(),
                        //     i.liquidity,
                        //     amount
                        // );
                        let chan_str = i.channel.short_channel_id.to_string();
                        !exclude.contains(&chan_str)
                            && !tempbans.contains_key(&chan_str)
                            && i.liquidity >= *amount
                            && Amount::msat(&i.channel.htlc_minimum_msat) <= *amount
                            && Amount::msat(
                                &i.channel.htlc_maximum_msat.unwrap_or(i.channel.amount_msat),
                            ) >= *amount
                            && !exclude_peers.contains(&i.channel.source)
                            && !exclude_peers.contains(&i.channel.destination)
                            && if i.channel.source == *mypubkey
                                || i.channel.destination == *mypubkey
                            {
                                candidatelist.iter().any(|c| c.to_string() == chan_str)
                            } else {
                                true
                            }
                    })
                    .collect::<Vec<&DirectedChannel>>();
            }
            None => return Vec::<&DirectedChannel>::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SuccessReb {
    pub amount_msat: u64,
    pub fee_ppm: u32,
    pub channel_partner: ShortChannelId,
    pub hops: u8,
    pub completed_at: u64,
}
impl SuccessReb {
    pub async fn write_to_file(
        &self,
        chan_id: ShortChannelId,
        sling_dir: &PathBuf,
    ) -> Result<(), Error> {
        let serialized = serde_json::to_string(self)?;
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(sling_dir.join(chan_id.to_string() + SUCCESSES_SUFFIX))
            .await?;
        file.write_all(format!("{}\n", serialized).as_bytes())
            .await?;
        Ok(())
    }

    pub async fn read_from_file(
        sling_dir: &PathBuf,
        chan_id: ShortChannelId,
    ) -> Result<Vec<SuccessReb>, Error> {
        let contents =
            tokio::fs::read_to_string(sling_dir.join(chan_id.to_string() + SUCCESSES_SUFFIX))
                .await?;
        let mut vec = vec![];
        for line in contents.lines() {
            vec.push(serde_json::from_str(line)?);
        }
        Ok(vec)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FailureReb {
    pub amount_msat: u64,
    pub failure_reason: String,
    pub failure_node: PublicKey,
    pub channel_partner: ShortChannelId,
    pub hops: u8,
    pub created_at: u64,
}
impl FailureReb {
    pub async fn write_to_file(
        &self,
        chan_id: ShortChannelId,
        sling_dir: &PathBuf,
    ) -> Result<(), Error> {
        let serialized = serde_json::to_string(self)?;
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(sling_dir.join(chan_id.to_string() + FAILURES_SUFFIX))
            .await?;
        file.write_all(format!("{}\n", serialized).as_bytes())
            .await?;
        Ok(())
    }

    pub async fn read_from_file(
        sling_dir: &PathBuf,
        chan_id: ShortChannelId,
    ) -> Result<Vec<FailureReb>, Error> {
        let contents =
            tokio::fs::read_to_string(sling_dir.join(chan_id.to_string() + FAILURES_SUFFIX))
                .await?;
        let mut vec = vec![];
        for line in contents.lines() {
            vec.push(serde_json::from_str(line)?);
        }
        Ok(vec)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelPartnerStats {
    pub scid: String,
    pub alias: String,
    pub sats: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerPartnerStats {
    pub peer_id: String,
    pub alias: String,
    pub count: u64,
}

#[derive(Debug, Tabled)]
pub struct StatSummary {
    pub alias: String,
    pub scid: String,
    pub pubkey: String,
    pub status: String,
    pub rebamount: String,
    pub w_feeppm: u64,
    pub last_route_taken: String,
    pub last_success_reb: String,
}
