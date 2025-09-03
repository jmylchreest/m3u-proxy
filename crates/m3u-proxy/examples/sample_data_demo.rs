//! Demo showing how the sample data generator works in test suites

use m3u_proxy::utils::SampleDataGenerator;

fn main() {
    println!("🎯 Sample Data Generator Demo");
    println!("{}", "=".repeat(50));
    
    let mut generator = SampleDataGenerator::new();
    
    // Example 1: Basic channel generation
    println!("\n📺 Basic Entertainment Channels:");
    let entertainment_channels = generator.generate_sample_channels(3, Some("entertainment"));
    for (i, channel) in entertainment_channels.iter().enumerate() {
        println!("  {}. {} (tvg-id: {}, group: {})", 
                 i+1, channel.channel_name, channel.tvg_id, channel.group_title);
    }
    
    // Example 2: Sports channels (some with timeshift)
    println!("\n🏆 Sports Channels (with random timeshift):");
    let sports_channels = generator.generate_sample_channels(4, Some("sports"));
    for (i, channel) in sports_channels.iter().enumerate() {
        println!("  {}. {} ({})", 
                 i+1, channel.channel_name, channel.tvg_id);
    }
    
    // Example 3: Adult channels for filter testing
    println!("\n🔞 Adult Channels (for filter testing):");
    let adult_channels = generator.generate_adult_channels(3);
    for (i, channel) in adult_channels.iter().enumerate() {
        println!("  {}. {} ({})", 
                 i+1, channel.channel_name, channel.tvg_id);
    }
    
    // Example 4: Specific timeshift channel generation
    println!("\n⏰ Guaranteed Timeshift Channels:");
    let timeshift_channels = generator.generate_timeshift_channels(3, Some("news"));
    for (i, channel) in timeshift_channels.iter().enumerate() {
        println!("  {}. {} ({})", i+1, channel.channel_name, channel.tvg_id);
    }
    
    println!("\n📺 Guaranteed Standard (Non-timeshift) Channels:");
    let standard_channels = generator.generate_standard_channels(3, Some("sports"));
    for (i, channel) in standard_channels.iter().enumerate() {
        println!("  {}. {} ({})", i+1, channel.channel_name, channel.tvg_id);
    }
    
    // Example 5: Custom timeshift ratio
    println!("\n🎛️  Custom 80% Timeshift Ratio:");
    let custom_ratio_channels = generator.generate_sample_channels_with_options(5, Some("movies"), Some(0.8));
    for (i, channel) in custom_ratio_channels.iter().enumerate() {
        let indicator = if channel.channel_name.contains("+") || channel.channel_name.contains("-") {
            "⏰"
        } else {
            "📺"
        };
        println!("  {}. {} {} ({})", i+1, indicator, channel.channel_name, channel.tvg_id);
    }
    
    // Example 6: Mixed categories like in integration tests
    println!("\n🎬 Mixed Categories (like in integration tests):");
    let news_samples = generator.generate_sample_channels(2, Some("news"));
    let movie_samples = generator.generate_sample_channels(2, Some("movies"));
    
    println!("  News Channels:");
    for channel in &news_samples {
        println!("    - {} ({})", channel.channel_name, channel.tvg_id);
    }
    
    println!("  Movie Channels:");
    for channel in &movie_samples {
        println!("    - {} ({})", channel.channel_name, channel.tvg_id);
    }
    
    // Example 7: Show full SampleChannel structure
    println!("\n📋 Full Channel Structure Example:");
    let sample = generator.generate_sample_channels(1, Some("documentary"))[0].clone();
    println!("  Channel Name: {}", sample.channel_name);
    println!("  TVG ID: {}", sample.tvg_id);
    println!("  TVG Name: {:?}", sample.tvg_name);
    println!("  TVG ChNo: {:?}", sample.tvg_chno);
    println!("  Group Title: {}", sample.group_title);
    println!("  TVG Logo: {:?}", sample.tvg_logo);
    println!("  Stream URL: {}", sample.stream_url);
    
    println!("\n✅ This replaces hardcoded real channel names in tests!");
}