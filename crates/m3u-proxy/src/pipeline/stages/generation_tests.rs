use quick_xml::Reader;
use quick_xml::events::Event;

/// Test utilities for XMLTV validation
pub struct XmltvTestUtils;

impl XmltvTestUtils {
    /// Parse and validate XMLTV content structure
    pub fn validate_xmltv_structure(
        content: &str,
    ) -> Result<XmltvValidationResult, Box<dyn std::error::Error>> {
        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        let mut validation_result = XmltvValidationResult::new();
        let mut buf = Vec::new();
        let mut current_element_stack = Vec::new();
        let mut in_tv_root = false;
        let mut channels_processed = 0;
        let mut programs_processed = 0;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                    current_element_stack.push(name.clone());

                    match name.as_str() {
                        "tv" => {
                            in_tv_root = true;
                            validation_result.has_tv_root = true;

                            // Validate TV root attributes
                            for attr in e.attributes() {
                                let attr = attr?;
                                let key = std::str::from_utf8(attr.key.as_ref())?;
                                let value = std::str::from_utf8(&attr.value)?;

                                match key {
                                    "date" => validation_result.has_date_attribute = true,
                                    "source-info-name" => {
                                        validation_result.has_source_info_name = true
                                    }
                                    "source-info-url" => {
                                        validation_result.has_source_info_url = true
                                    }
                                    "generator-info-name" => {
                                        validation_result.has_generator_info_name = true
                                    }
                                    "generator-info-url" => {
                                        validation_result.has_generator_info_url = true
                                    }
                                    _ => {}
                                }

                                validation_result
                                    .tv_attributes
                                    .insert(key.to_string(), value.to_string());
                            }
                        }
                        "channel" => {
                            if in_tv_root {
                                channels_processed += 1;

                                // Extract channel id
                                for attr in e.attributes() {
                                    let attr = attr?;
                                    let key = std::str::from_utf8(attr.key.as_ref())?;
                                    let value = std::str::from_utf8(&attr.value)?;

                                    if key == "id" {
                                        validation_result.channel_ids.push(value.to_string());
                                    }
                                }
                            }
                        }
                        "programme" => {
                            if in_tv_root {
                                programs_processed += 1;

                                // Extract programme attributes
                                let mut programme_info = ProgrammeInfo::default();
                                for attr in e.attributes() {
                                    let attr = attr?;
                                    let key = std::str::from_utf8(attr.key.as_ref())?;
                                    let value = std::str::from_utf8(&attr.value)?;

                                    match key {
                                        "channel" => {
                                            programme_info.channel_id = Some(value.to_string())
                                        }
                                        "start" => {
                                            programme_info.start_time = Some(value.to_string())
                                        }
                                        "stop" => programme_info.end_time = Some(value.to_string()),
                                        _ => {}
                                    }
                                }
                                validation_result.programmes.push(programme_info);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                    current_element_stack.pop();

                    if name == "tv" {
                        in_tv_root = false;
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = std::str::from_utf8(e)?;
                    if !text.trim().is_empty() && !current_element_stack.is_empty() {
                        let current_element = current_element_stack.last().unwrap();

                        // Capture text content for validation
                        if current_element == "title" && current_element_stack.len() >= 2 {
                            let parent = &current_element_stack[current_element_stack.len() - 2];
                            if parent == "programme"
                                && let Some(last_programme) =
                                    validation_result.programmes.last_mut()
                            {
                                last_programme.title = Some(text.trim().to_string());
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(format!("XML parsing error: {}", e).into()),
                _ => {}
            }

            buf.clear();
        }

        validation_result.channels_count = channels_processed;
        validation_result.programmes_count = programs_processed;
        validation_result.is_valid_xmltv = validation_result.validate();

        Ok(validation_result)
    }
}

/// XMLTV validation result
#[derive(Debug, Default)]
pub struct XmltvValidationResult {
    pub is_valid_xmltv: bool,
    pub has_tv_root: bool,
    pub has_date_attribute: bool,
    pub has_source_info_name: bool,
    pub has_source_info_url: bool,
    pub has_generator_info_name: bool,
    pub has_generator_info_url: bool,
    pub tv_attributes: std::collections::HashMap<String, String>,
    pub channels_count: usize,
    pub programmes_count: usize,
    pub channel_ids: Vec<String>,
    pub programmes: Vec<ProgrammeInfo>,
}

#[derive(Debug, Default)]
pub struct ProgrammeInfo {
    pub channel_id: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub title: Option<String>,
}

impl XmltvValidationResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn validate(&self) -> bool {
        self.has_tv_root && self.has_date_attribute && self.has_source_info_name
        // channels_count is usize, so always >= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XMLTV: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE tv SYSTEM "xmltv.dtd">
<tv date="01/01/2024 12:00:00" source-info-url="https://github.com/jmylchreest/m3u-proxy" source-info-name="m3u-proxy" generator-info-name="m3u-proxy" generator-info-url="https://github.com/jmylchreest/m3u-proxy">
  <channel id="bbc-one-hd">
    <display-name>BBC One HD</display-name>
    <icon src="https://example.com/bbc-one.png"/>
  </channel>
  <channel id="cnn-intl">
    <display-name>CNN International</display-name>
    <icon src="https://example.com/cnn.png"/>
  </channel>
  <programme channel="bbc-one-hd" start="20240101180000 +0000" stop="20240101183000 +0000">
    <title>BBC News at Six</title>
    <desc>The latest national and international news stories</desc>
    <category>News</category>
    <sub-title>English subtitles</sub-title>
    <language>en</language>
    <rating>TV-G</rating>
  </programme>
  <programme channel="cnn-intl" start="20240101120000 +0000" stop="20240101130000 +0000">
    <title>Breaking News Special</title>
    <desc>Live coverage of breaking news events</desc>
    <category>News</category>
    <language>en</language>
    <rating>TV-PG</rating>
  </programme>
</tv>"#;

    #[test]
    fn test_xmltv_structure_validation() {
        let validation_result = XmltvTestUtils::validate_xmltv_structure(SAMPLE_XMLTV)
            .expect("Should parse sample XMLTV");

        // Validate basic structure
        assert!(validation_result.is_valid_xmltv, "XMLTV should be valid");
        assert!(
            validation_result.has_tv_root,
            "Should have <tv> root element"
        );
        assert!(
            validation_result.has_date_attribute,
            "Should have date attribute"
        );
        assert!(
            validation_result.has_source_info_name,
            "Should have source-info-name"
        );
        assert!(
            validation_result.has_generator_info_name,
            "Should have generator-info-name"
        );

        // Validate channels
        assert_eq!(
            validation_result.channels_count, 2,
            "Should have 2 channels"
        );
        assert!(
            validation_result
                .channel_ids
                .contains(&"bbc-one-hd".to_string())
        );
        assert!(
            validation_result
                .channel_ids
                .contains(&"cnn-intl".to_string())
        );

        // Validate programmes
        assert_eq!(
            validation_result.programmes_count, 2,
            "Should have 2 programmes"
        );

        let bbc_programme = validation_result
            .programmes
            .iter()
            .find(|p| p.channel_id == Some("bbc-one-hd".to_string()))
            .expect("Should find BBC programme");
        assert_eq!(bbc_programme.title, Some("BBC News at Six".to_string()));
        assert!(bbc_programme.start_time.is_some());
        assert!(bbc_programme.end_time.is_some());
    }

    #[test]
    fn test_xmltv_missing_required_attributes() {
        let invalid_xmltv = r#"<?xml version="1.0" encoding="UTF-8"?>
<tv>
  <channel id="test">
    <display-name>Test</display-name>
  </channel>
</tv>"#;

        let validation_result = XmltvTestUtils::validate_xmltv_structure(invalid_xmltv)
            .expect("Should parse even invalid XMLTV");

        assert!(
            !validation_result.is_valid_xmltv,
            "Should be invalid without required attributes"
        );
        assert!(
            !validation_result.has_date_attribute,
            "Should be missing date attribute"
        );
        assert!(
            !validation_result.has_source_info_name,
            "Should be missing source-info-name"
        );
    }

    #[test]
    fn test_xmltv_xml_escaping_validation() {
        let escaped_xmltv = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE tv SYSTEM "xmltv.dtd">
<tv date="01/01/2024 12:00:00" source-info-name="m3u-proxy" generator-info-name="m3u-proxy">
  <channel id="test-channel">
    <display-name>Test &amp; Channel</display-name>
  </channel>
  <programme channel="test-channel" start="20240101120000 +0000" stop="20240101130000 +0000">
    <title>News &amp; Views: &quot;Breaking&quot; &lt;Live&gt;</title>
    <desc>Description with &lt;tags&gt; &amp; &quot;quotes&quot;</desc>
    <category>News &amp; Current Affairs</category>
  </programme>
</tv>"#;

        let validation_result = XmltvTestUtils::validate_xmltv_structure(escaped_xmltv)
            .expect("Should parse XMLTV with escaped characters");

        assert!(
            validation_result.is_valid_xmltv,
            "Should be valid despite escaped characters"
        );
        assert_eq!(validation_result.channels_count, 1);
        assert_eq!(validation_result.programmes_count, 1);

        let programme = &validation_result.programmes[0];
        // The XML parser will decode the escaped characters back to their original form
        // So we verify the escaped content was properly parsed
        assert!(programme.title.is_some());
        assert_eq!(programme.channel_id, Some("test-channel".to_string()));
    }

    #[test]
    fn test_xmltv_empty_programmes() {
        let empty_programmes_xmltv = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE tv SYSTEM "xmltv.dtd">
<tv date="01/01/2024 12:00:00" source-info-name="m3u-proxy" generator-info-name="m3u-proxy">
  <channel id="empty-channel">
    <display-name>Empty Channel</display-name>
  </channel>
</tv>"#;

        let validation_result = XmltvTestUtils::validate_xmltv_structure(empty_programmes_xmltv)
            .expect("Should parse XMLTV with no programmes");

        assert!(
            validation_result.is_valid_xmltv,
            "Should be valid with no programmes"
        );
        assert_eq!(validation_result.channels_count, 1);
        assert_eq!(validation_result.programmes_count, 0);
    }

    #[test]
    fn test_xmltv_programme_attributes_validation() {
        let programme_attrs_xmltv = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE tv SYSTEM "xmltv.dtd">
<tv date="01/01/2024 12:00:00" source-info-name="m3u-proxy" generator-info-name="m3u-proxy">
  <channel id="attrs-test">
    <display-name>Attributes Test</display-name>
  </channel>
  <programme channel="attrs-test" start="20240101180000 +0000" stop="20240101190000 +0000">
    <title>Test Programme</title>
  </programme>
</tv>"#;

        let validation_result = XmltvTestUtils::validate_xmltv_structure(programme_attrs_xmltv)
            .expect("Should parse XMLTV programme attributes");

        let programme = &validation_result.programmes[0];
        assert_eq!(programme.channel_id, Some("attrs-test".to_string()));
        assert_eq!(
            programme.start_time,
            Some("20240101180000 +0000".to_string())
        );
        assert_eq!(programme.end_time, Some("20240101190000 +0000".to_string()));
        assert_eq!(programme.title, Some("Test Programme".to_string()));
    }
}
