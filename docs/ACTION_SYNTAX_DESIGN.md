# Action Syntax Patterns Design

## Overview

This document details the specific design patterns for action syntax in the extended filter system, including data structures, parsing strategies, and implementation patterns.

## Core Action Data Structures

### Rust Implementation

```rust
// Extended expression that can contain both conditions and actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExtendedExpression {
    ConditionOnly(ConditionTree),
    ConditionWithActions {
        condition: ConditionTree,
        actions: Vec<Action>,
    },
}

// Individual action within a SET clause
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub field: String,
    pub operator: ActionOperator,
    pub value: ActionValue,
}

// Assignment operators for actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionOperator {
    #[serde(rename = "set")]
    Set,        // = (overwrite)
    
    #[serde(rename = "append")]
    Append,     // += (append with space)
    
    #[serde(rename = "set_if_empty")]
    SetIfEmpty, // ?= (set only if empty)
    
    #[serde(rename = "remove")]
    Remove,     // -= (remove substring)
}

// Values that can be assigned in actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionValue {
    #[serde(rename = "literal")]
    Literal(String),
    
    #[serde(rename = "function")]
    Function(FunctionCall),    // Future: upper(), trim(), etc.
    
    #[serde(rename = "variable")]
    Variable(VariableRef),     // Future: $field_name
}

// Future: Function call support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Vec<ActionValue>,
}

// Future: Variable reference support
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableRef {
    pub field_name: String,
}
```

## Action Operator Behavior

### Set Operator (`=`)
**Purpose**: Completely replace the field value

**Behavior**:
- Overwrites existing value regardless of current content
- Sets null/empty fields to the specified value
- Most common and straightforward operator

**Examples**:
```
group_title equals "" SET group_title = "Uncategorized"
channel_name contains "bbc" SET tvg_logo = "bbc-logo.png"
language equals "" SET language = "en"
```

**Implementation**:
```rust
fn apply_set_action(channel: &mut Channel, field: &str, value: &str) -> Result<()> {
    match field {
        "group_title" => channel.group_title = Some(value.to_string()),
        "tvg_logo" => channel.tvg_logo = Some(value.to_string()),
        "language" => channel.language = Some(value.to_string()),
        _ => return Err(anyhow!("Unknown field: {}", field)),
    }
    Ok(())
}
```

### Append Operator (`+=`)
**Purpose**: Add content to existing field value

**Behavior**:
- If field is empty/null: sets to the new value
- If field has content: adds a space + new value
- Useful for adding tags, suffixes, or additional information

**Examples**:
```
channel_name contains "HD" SET channel_name += "[High Definition]"
group_title contains "sport" SET group_title += " - Premium"
tvg_name equals "" SET tvg_name += channel_name
```

**Implementation**:
```rust
fn apply_append_action(channel: &mut Channel, field: &str, value: &str) -> Result<()> {
    let current_value = get_field_value(channel, field)?;
    let new_value = if current_value.is_empty() {
        value.to_string()
    } else {
        format!("{} {}", current_value, value)
    };
    set_field_value(channel, field, &new_value)
}
```

### Set If Empty Operator (`?=`)
**Purpose**: Set value only when field is currently empty

**Behavior**:
- If field is null, empty string, or whitespace-only: sets the value
- If field has any non-whitespace content: no action taken
- Perfect for providing default values

**Examples**:
```
group_title equals "" SET group_title ?= "General"
tvg_logo equals "" SET tvg_logo ?= "default-logo.png"
language matches "^\s*$" SET language ?= "en"
```

**Implementation**:
```rust
fn apply_set_if_empty_action(channel: &mut Channel, field: &str, value: &str) -> Result<()> {
    let current_value = get_field_value(channel, field)?;
    if current_value.trim().is_empty() {
        set_field_value(channel, field, value)?;
    }
    Ok(())
}
```

### Remove Operator (`-=`)
**Purpose**: Remove specified substring from field value

**Behavior**:
- Removes all occurrences of the substring (case-sensitive)
- If substring not found: no change to field
- Useful for cleaning up channel names, removing tags

**Examples**:
```
channel_name contains "[AD]" SET channel_name -= "[AD]"
channel_name contains "  " SET channel_name -= "  "  # Remove double spaces
group_title ends_with " HD" SET group_title -= " HD"
```

**Implementation**:
```rust
fn apply_remove_action(channel: &mut Channel, field: &str, value: &str) -> Result<()> {
    let current_value = get_field_value(channel, field)?;
    let new_value = current_value.replace(value, "");
    set_field_value(channel, field, &new_value)
}
```

## Field Access Patterns

### Generic Field Accessor
```rust
fn get_field_value(channel: &Channel, field: &str) -> Result<String> {
    match field {
        "channel_name" => Ok(channel.channel_name.clone()),
        "group_title" => Ok(channel.group_title.clone().unwrap_or_default()),
        "tvg_id" => Ok(channel.tvg_id.clone().unwrap_or_default()),
        "tvg_name" => Ok(channel.tvg_name.clone().unwrap_or_default()),
        "tvg_logo" => Ok(channel.tvg_logo.clone().unwrap_or_default()),
        "tvg_shift" => Ok(channel.tvg_shift.clone().unwrap_or_default()),
        "stream_url" => Ok(channel.stream_url.clone()),
        _ => Err(anyhow!("Unknown field: {}", field)),
    }
}

fn set_field_value(channel: &mut Channel, field: &str, value: &str) -> Result<()> {
    match field {
        "channel_name" => channel.channel_name = value.to_string(),
        "group_title" => channel.group_title = Some(value.to_string()),
        "tvg_id" => channel.tvg_id = Some(value.to_string()),
        "tvg_name" => channel.tvg_name = Some(value.to_string()),
        "tvg_logo" => channel.tvg_logo = Some(value.to_string()),
        "tvg_shift" => channel.tvg_shift = Some(value.to_string()),
        "stream_url" => channel.stream_url = value.to_string(),
        _ => return Err(anyhow!("Unknown field: {}", field)),
    }
    Ok(())
}
```

## Parsing Strategy

### Token Extensions
```rust
#[derive(Debug, Clone)]
enum Token {
    // Existing tokens
    Field(String),
    Operator(FilterOperator),
    Value(String),
    LogicalOp(LogicalOperator),
    Modifier(String),
    LeftParen,
    RightParen,
    
    // New action tokens
    SetKeyword,              // SET
    AssignmentOp(ActionOperator), // =, +=, ?=, -=
    Comma,                   // ,
}
```

### Extended Tokenizer Logic
```rust
// In tokenize() method - new token recognition
if remaining.to_uppercase().starts_with("SET") {
    let end_pos = 3;
    if end_pos == remaining.len() || remaining.chars().nth(end_pos).map_or(true, |c| c.is_whitespace()) {
        tokens.push(Token::SetKeyword);
        current_pos += end_pos;
        continue;
    }
}

// Assignment operators
if remaining.starts_with("+=") {
    tokens.push(Token::AssignmentOp(ActionOperator::Append));
    current_pos += 2;
    continue;
}
if remaining.starts_with("?=") {
    tokens.push(Token::AssignmentOp(ActionOperator::SetIfEmpty));
    current_pos += 2;
    continue;
}
if remaining.starts_with("-=") {
    tokens.push(Token::AssignmentOp(ActionOperator::Remove));
    current_pos += 2;
    continue;
}
if remaining.starts_with("=") {
    tokens.push(Token::AssignmentOp(ActionOperator::Set));
    current_pos += 1;
    continue;
}
if remaining.starts_with(",") {
    tokens.push(Token::Comma);
    current_pos += 1;
    continue;
}
```

### Extended Parser Logic
```rust
impl FilterParser {
    pub fn parse_extended(&self, expression: &str) -> Result<ExtendedExpression> {
        let tokens = self.tokenize(expression)?;
        let mut pos = 0;
        
        // Parse condition expression first
        let condition = self.parse_condition_expression(&tokens, &mut pos)?;
        
        // Check for SET keyword
        if pos < tokens.len() && matches!(tokens[pos], Token::SetKeyword) {
            pos += 1; // consume SET
            let actions = self.parse_action_list(&tokens, &mut pos)?;
            Ok(ExtendedExpression::ConditionWithActions { condition, actions })
        } else {
            Ok(ExtendedExpression::ConditionOnly(condition))
        }
    }
    
    fn parse_action_list(&self, tokens: &[Token], pos: &mut usize) -> Result<Vec<Action>> {
        let mut actions = Vec::new();
        
        loop {
            let action = self.parse_action(tokens, pos)?;
            actions.push(action);
            
            if *pos < tokens.len() && matches!(tokens[*pos], Token::Comma) {
                *pos += 1; // consume comma
                continue;
            } else {
                break;
            }
        }
        
        Ok(actions)
    }
    
    fn parse_action(&self, tokens: &[Token], pos: &mut usize) -> Result<Action> {
        // Parse field name
        let field = match &tokens[*pos] {
            Token::Field(name) => name.clone(),
            _ => return Err(anyhow!("Expected field name in action")),
        };
        *pos += 1;
        
        // Parse assignment operator
        let operator = match &tokens[*pos] {
            Token::AssignmentOp(op) => op.clone(),
            _ => return Err(anyhow!("Expected assignment operator")),
        };
        *pos += 1;
        
        // Parse value
        let value = match &tokens[*pos] {
            Token::Value(val) => ActionValue::Literal(val.clone()),
            _ => return Err(anyhow!("Expected value in action")),
        };
        *pos += 1;
        
        Ok(Action { field, operator, value })
    }
}
```

## Complex Action Patterns

### Chained Actions
```
channel_name contains "sport" SET 
    group_title = "Sports", 
    category = "entertainment", 
    tvg_logo ?= "sports-default.png"
```

### Conditional Actions
```
tvg_id equals "" SET tvg_id ?= channel_name
group_title equals "" SET group_title ?= "General"
```

### Cleanup Actions
```
channel_name matches ".*\[AD\].*" SET 
    channel_name -= "[AD]",
    channel_name -= "  ",
    group_title += " (Audio Description)"
```

### Logo Assignment Patterns
```
tvg_id starts_with "bbc" SET tvg_logo = "https://logos.example.com/bbc.png"
tvg_id starts_with "itv" SET tvg_logo = "https://logos.example.com/itv.png"
channel_name contains "4K" SET tvg_logo += "-4k"
```

## Validation Strategies

### Syntax Validation
1. **Action structure**: Field + operator + value format
2. **Comma separation**: Proper action list formatting
3. **Quote matching**: Values properly quoted
4. **Operator validity**: Assignment operators exist and are recognized

### Semantic Validation
1. **Field existence**: All action fields must exist for the source type
2. **Operator compatibility**: Some operators may not work with certain field types
3. **Value constraints**: Values must meet field-specific requirements
4. **Circular references**: Actions cannot reference themselves

### Runtime Validation
1. **String length limits**: Respect database field constraints
2. **URL format validation**: For logo and stream URL fields
3. **ID format validation**: For tvg_id and similar identifier fields
4. **Character set validation**: Ensure no invalid characters

## Error Handling Examples

### Syntax Errors
```
Input: "channel_name contains 'sport' SET group_title == 'Sports'"
Error: "Invalid assignment operator '==' at position 42. Did you mean '='?"

Input: "channel_name contains 'sport' SET group_title = Sports"
Error: "Unquoted value 'Sports' at position 50. Values must be quoted."

Input: "channel_name contains 'sport' SET group_title = 'Sports' category = 'TV'"
Error: "Missing comma between actions at position 62."
```

### Semantic Errors
```
Input: "channel_name contains 'sport' SET invalid_field = 'Sports'"
Error: "Unknown field 'invalid_field' for stream sources. Available fields: channel_name, group_title, tvg_id, tvg_name, tvg_logo, tvg_shift, stream_url"

Input: "channel_name contains 'sport' SET tvg_shift += 'extra'"
Error: "Operator '+=' not recommended for field 'tvg_shift'. Consider using '=' instead."
```

This action syntax design provides a robust foundation for implementing the data transformation capabilities while maintaining the elegant natural language approach of the existing filter system.