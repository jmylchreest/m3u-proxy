//! Sample data generator for tests and documentation
//! 
//! Provides utilities to generate realistic but fictional channel names, broadcaster names,
//! and other test data to avoid using real brand names in tests and documentation.

use rand::seq::IndexedRandom;
use rand::{Rng, rng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleChannelData {
    pub broadcasters: Vec<String>,
    pub channel_variants: ChannelVariants,
    pub international: HashMap<String, Vec<InternationalBroadcaster>>,
    pub special_formats: Vec<SpecialFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelVariants {
    pub standard: Vec<String>,
    pub quality: Vec<String>,
    pub timeshift: Vec<String>,
    pub categories: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternationalBroadcaster {
    pub broadcaster: String,
    pub channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecialFormat {
    pub pattern: String,
    pub examples: Vec<String>,
}

/// Sample data generator for creating realistic fictional channel data
pub struct SampleDataGenerator {
    data: SampleChannelData,
    rng: rand::rngs::ThreadRng,
}

impl Default for SampleDataGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl SampleDataGenerator {
    /// Create a new sample data generator with embedded data
    pub fn new() -> Self {
        let data = Self::load_embedded_data();
        Self {
            data,
            rng: rng(),
        }
    }

    /// Load the embedded sample channel data
    fn load_embedded_data() -> SampleChannelData {
        // In a real implementation, this would load from the JSON file
        // For now, we'll embed some basic data
        SampleChannelData {
            broadcasters: vec![
                "StreamCast".to_string(),
                "ViewMedia".to_string(),
                "AeroVision".to_string(),
                "GlobalStream".to_string(),
                "NationalNet".to_string(),
                "SportsCentral".to_string(),
                "CinemaMax".to_string(),
                "MusicMax".to_string(),
            ],
            channel_variants: ChannelVariants {
                standard: vec![
                    "One".to_string(),
                    "Two".to_string(),
                    "Three".to_string(),
                    "Prime".to_string(),
                    "Plus".to_string(),
                    "Max".to_string(),
                ],
                quality: vec![
                    "HD".to_string(),
                    "SD".to_string(),
                    "4K".to_string(),
                    "UHD".to_string(),
                ],
                timeshift: vec![
                    "+1".to_string(),
                    "+2".to_string(),
                    "+24".to_string(),
                    "+1h".to_string(),
                ],
                categories: {
                    let mut cats = HashMap::new();
                    cats.insert("news".to_string(), vec![
                        "News".to_string(),
                        "News HD".to_string(),
                        "Breaking News".to_string(),
                        "World News".to_string(),
                    ]);
                    cats.insert("sports".to_string(), vec![
                        "Sports".to_string(),
                        "Sports HD".to_string(),
                        "Racing HD".to_string(),
                        "Football HD".to_string(),
                    ]);
                    cats.insert("movies".to_string(), vec![
                        "Movies".to_string(),
                        "Movies HD".to_string(),
                        "Action Movies HD".to_string(),
                        "Classic Movies".to_string(),
                    ]);
                    cats.insert("adult".to_string(), vec![
                        "Adult Channel".to_string(),
                        "XXX Movies".to_string(),
                        "Porn Central".to_string(),
                        "Adult Entertainment".to_string(),
                    ]);
                    cats
                },
            },
            international: HashMap::new(),
            special_formats: vec![],
        }
    }

    /// Get a random broadcaster name
    pub fn random_broadcaster(&mut self) -> String {
        self.data.broadcasters
            .choose(&mut self.rng)
            .unwrap_or(&"StreamCast".to_string())
            .clone()
    }

    /// Get a random channel from a specific category
    pub fn random_channel_from_category(&mut self, category: &str) -> String {
        if let Some(channels) = self.data.channel_variants.categories.get(category) {
            channels
                .choose(&mut self.rng)
                .unwrap_or(&"Generic Channel".to_string())
                .clone()
        } else {
            "Generic Channel".to_string()
        }
    }

    /// Generate a full channel name with broadcaster
    pub fn generate_channel_name(&mut self, category: Option<&str>) -> String {
        let broadcaster = self.random_broadcaster();
        let channel = if let Some(cat) = category {
            self.random_channel_from_category(cat)
        } else {
            // Pick a random category
            let category_keys: Vec<String> = self.data.channel_variants.categories.keys().cloned().collect();
            let random_category_key = category_keys.choose(&mut self.rng).unwrap();
            self.random_channel_from_category(random_category_key)
        };
        
        format!("{} {}", broadcaster, channel)
    }

    /// Generate a channel name with timeshift
    pub fn generate_timeshift_channel(&mut self, category: Option<&str>) -> String {
        let base_channel = self.generate_channel_name(category);
        let timeshift = self.data.channel_variants.timeshift
            .choose(&mut self.rng)
            .unwrap();
        
        format!("{} {}", base_channel, timeshift)
    }

    /// Generate multiple sample channels for testing
    pub fn generate_sample_channels(&mut self, count: usize, category: Option<&str>) -> Vec<SampleChannel> {
        self.generate_sample_channels_with_options(count, category, None)
    }
    
    /// Generate sample channels with explicit timeshift control
    /// 
    /// # Arguments
    /// * `count` - Number of channels to generate
    /// * `category` - Optional category filter ("sports", "news", "adult", etc.)
    /// * `timeshift_ratio` - Optional ratio of timeshift channels (0.0-1.0). 
    ///   If None, uses default 20% chance. If Some(1.0), all channels will be timeshift.
    pub fn generate_sample_channels_with_options(
        &mut self, 
        count: usize, 
        category: Option<&str>,
        timeshift_ratio: Option<f64>
    ) -> Vec<SampleChannel> {
        let timeshift_probability = timeshift_ratio.unwrap_or(0.2);
        
        (0..count)
            .map(|i| {
                let channel_name = if self.rng.random_bool(timeshift_probability) {
                    self.generate_timeshift_channel(category)
                } else {
                    self.generate_channel_name(category)
                };

                let group_title = match category {
                    Some(cat) => {
                        // Capitalize first letter for consistency
                        let mut chars: Vec<char> = cat.chars().collect();
                        if !chars.is_empty() {
                            chars[0] = chars[0].to_uppercase().next().unwrap_or(chars[0]);
                        }
                        chars.into_iter().collect()
                    },
                    None => "Entertainment".to_string()
                };

                SampleChannel {
                    tvg_id: format!("ch{:03}", i + 1),
                    tvg_name: Some(channel_name.clone()),
                    tvg_chno: Some(format!("{}", i + 101)),
                    channel_name,
                    tvg_logo: Some(format!("https://logos.example.com/channel{}.png", i + 1)),
                    group_title,
                    stream_url: format!("https://stream.example.com/channel{}", i + 1),
                }
            })
            .collect()
    }
    
    /// Generate channels that are guaranteed to be timeshift channels
    pub fn generate_timeshift_channels(&mut self, count: usize, category: Option<&str>) -> Vec<SampleChannel> {
        self.generate_sample_channels_with_options(count, category, Some(1.0))
    }
    
    /// Generate channels that are guaranteed to be non-timeshift channels
    pub fn generate_standard_channels(&mut self, count: usize, category: Option<&str>) -> Vec<SampleChannel> {
        self.generate_sample_channels_with_options(count, category, Some(0.0))
    }

    /// Generate adult content channels specifically for filter testing
    pub fn generate_adult_channels(&mut self, count: usize) -> Vec<SampleChannel> {
        self.generate_sample_channels(count, Some("adult"))
    }

    /// Generate sports channels with HD variants
    pub fn generate_sports_channels(&mut self, count: usize) -> Vec<SampleChannel> {
        self.generate_sample_channels(count, Some("sports"))
    }

    /// Generate news channels
    pub fn generate_news_channels(&mut self, count: usize) -> Vec<SampleChannel> {
        self.generate_sample_channels(count, Some("news"))
    }
}

/// Sample channel structure for testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleChannel {
    pub tvg_id: String,
    pub tvg_name: Option<String>,
    pub tvg_chno: Option<String>,
    pub channel_name: String,
    pub tvg_logo: Option<String>,
    pub group_title: String,
    pub stream_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_broadcaster() {
        let mut generator = SampleDataGenerator::new();
        let broadcaster = generator.random_broadcaster();
        assert!(!broadcaster.is_empty());
        println!("Random broadcaster: {}", broadcaster);
    }

    #[test]
    fn test_generate_channel_name() {
        let mut generator = SampleDataGenerator::new();
        let channel = generator.generate_channel_name(Some("sports"));
        assert!(channel.contains("Sports") || channel.contains("Racing") || channel.contains("Football"));
        println!("Sample sports channel: {}", channel);
    }

    #[test]
    fn test_generate_timeshift_channel() {
        let mut generator = SampleDataGenerator::new();
        let channel = generator.generate_timeshift_channel(Some("news"));
        assert!(channel.contains("+1") || channel.contains("+2") || channel.contains("+24") || channel.contains("+1h"));
        println!("Sample timeshift channel: {}", channel);
    }

    #[test]
    fn test_generate_adult_channels() {
        let mut generator = SampleDataGenerator::new();
        let channels = generator.generate_adult_channels(3);
        assert_eq!(channels.len(), 3);
        
        for channel in &channels {
            assert!(channel.channel_name.contains("Adult") || 
                   channel.channel_name.contains("XXX") || 
                   channel.channel_name.contains("Porn"));
        }
    }

    #[test]
    fn test_generate_sample_channels() {
        let mut generator = SampleDataGenerator::new();
        let channels = generator.generate_sample_channels(10, Some("movies"));
        assert_eq!(channels.len(), 10);
        
        for (i, channel) in channels.iter().enumerate() {
            assert_eq!(channel.tvg_id, format!("ch{:03}", i + 1));
            assert!(channel.channel_name.contains("Movies") || channel.channel_name.contains("Cinema"));
            assert!(channel.stream_url.contains("stream.example.com"));
        }
    }
    
    #[test]
    fn test_generate_timeshift_channels() {
        let mut generator = SampleDataGenerator::new();
        let timeshift_channels = generator.generate_timeshift_channels(5, Some("sports"));
        assert_eq!(timeshift_channels.len(), 5);
        
        // All channels should have timeshift indicators
        for channel in &timeshift_channels {
            assert!(channel.channel_name.contains("+1") || 
                   channel.channel_name.contains("+2") || 
                   channel.channel_name.contains("+24") || 
                   channel.channel_name.contains("-1") ||
                   channel.channel_name.contains("+1h") ||
                   channel.channel_name.contains("+6h") ||
                   channel.channel_name.contains("+24h") ||
                   channel.channel_name.contains("-1h"),
                   "Channel '{}' should have timeshift indicator", channel.channel_name);
        }
        println!("Generated timeshift channels: {:?}", 
                timeshift_channels.iter().map(|c| &c.channel_name).collect::<Vec<_>>());
    }
    
    #[test]
    fn test_generate_standard_channels() {
        let mut generator = SampleDataGenerator::new();
        let standard_channels = generator.generate_standard_channels(5, Some("news"));
        assert_eq!(standard_channels.len(), 5);
        
        // No channels should have timeshift indicators
        for channel in &standard_channels {
            assert!(!channel.channel_name.contains("+1") && 
                   !channel.channel_name.contains("+2") && 
                   !channel.channel_name.contains("+24") && 
                   !channel.channel_name.contains("-1") &&
                   !channel.channel_name.contains("+1h") &&
                   !channel.channel_name.contains("+6h") &&
                   !channel.channel_name.contains("+24h") &&
                   !channel.channel_name.contains("-1h"),
                   "Channel '{}' should not have timeshift indicator", channel.channel_name);
        }
        println!("Generated standard channels: {:?}", 
                standard_channels.iter().map(|c| &c.channel_name).collect::<Vec<_>>());
    }
    
    #[test]
    fn test_generate_with_custom_timeshift_ratio() {
        let mut generator = SampleDataGenerator::new();
        
        // Test 100% timeshift ratio (guaranteed)
        let all_timeshift = generator.generate_sample_channels_with_options(5, Some("sports"), Some(1.0));
        assert_eq!(all_timeshift.len(), 5);
        
        let timeshift_count = all_timeshift.iter()
            .filter(|ch| ch.channel_name.contains("+") || ch.channel_name.contains("-"))
            .count();
            
        assert_eq!(timeshift_count, 5, "All channels should be timeshift with 1.0 ratio");
        
        // Test 0% timeshift ratio (guaranteed none)
        let no_timeshift = generator.generate_sample_channels_with_options(5, Some("news"), Some(0.0));
        let no_timeshift_count = no_timeshift.iter()
            .filter(|ch| ch.channel_name.contains("+") || ch.channel_name.contains("-"))
            .count();
            
        assert_eq!(no_timeshift_count, 0, "No channels should be timeshift with 0.0 ratio");
        
        println!("Generated {} timeshift channels out of 5 with 100% ratio", timeshift_count);
        println!("Generated {} timeshift channels out of 5 with 0% ratio", no_timeshift_count);
    }
}