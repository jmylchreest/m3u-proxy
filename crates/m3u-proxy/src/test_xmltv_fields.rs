// Test module to inspect xmltv Programme struct
use xmltv::Programme;

pub fn test_programme_fields() {
    let program = Programme {
        channel: "test".to_string(),
        start: "20240101120000".to_string(),
        stop: Some("20240101130000".to_string()),
        ..Default::default()
    };
    
    println!("Programme struct fields:");
    println!("  channel: {}", program.channel);
    println!("  start: {}", program.start);
    println!("  stop: {:?}", program.stop);
    println!("  titles: {:?}", program.titles);
    println!("  descriptions: {:?}", program.descriptions);
    println!("  categories: {:?}", program.categories);
    println!("  ratings: {:?}", program.ratings);
    println!("  episode_num: {:?}", program.episode_num);
    println!("  language: {:?}", program.language);
    
    // Check if icons field exists by trying to access it
    // This will cause a compile error if the field doesn't exist
    // println!("  icons: {:?}", program.icons);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explore_programme_struct() {
        test_programme_fields();
    }
}