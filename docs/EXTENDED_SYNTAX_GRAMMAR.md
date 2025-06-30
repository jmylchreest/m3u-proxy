# Extended Filter Syntax Grammar Specification

## Overview

This document defines the extended syntax grammar that combines the existing filter condition syntax with new action capabilities, enabling both filtering and data transformation in a unified natural language interface.

## Current Filter Grammar (Preserved)

### Basic Structure
```
expression := condition_expression
condition_expression := condition | group | condition logical_operator condition_expression
group := "(" condition_expression ")"
condition := [modifiers] field operator value
```

### Tokens
```
field := identifier
operator := "contains" | "equals" | "matches" | "starts_with" | "ends_with" | 
           "not_contains" | "not_equals" | "not_matches"
value := quoted_string
logical_operator := "AND" | "OR"
modifiers := ["not"] ["case_sensitive"]
quoted_string := "\"" string "\"" | "'" string "'"
```

## Extended Grammar with Actions

### Extended Structure
```
extended_expression := condition_expression [action_clause]
action_clause := "SET" action_list
action_list := action ("," action)*
action := field assignment_operator action_value
assignment_operator := "=" | "+=" | "?=" | "-="
action_value := quoted_string | function_call | variable_reference
```

### New Tokens
```
action_keyword := "SET"
assignment_operator := "=" | "+=" | "?=" | "-="
function_call := function_name "(" [argument_list] ")"
function_name := identifier
argument_list := action_value ("," action_value)*
variable_reference := "$" identifier | "${" field "}"
```

## Complete EBNF Grammar

```ebnf
(* Root expression *)
extended_expression = condition_expression [ action_clause ] ;

(* Condition expressions (existing) *)
condition_expression = logical_term { logical_operator logical_term } ;
logical_term = condition | group ;
group = "(" condition_expression ")" ;
condition = [ modifiers ] field operator value ;
modifiers = [ "not" ] [ "case_sensitive" ] ;

(* Action clause (new) *)
action_clause = "SET" action_list ;
action_list = action { "," action } ;
action = field assignment_operator action_value ;
assignment_operator = "=" | "+=" | "?=" | "-=" ;
action_value = quoted_string | function_call | variable_reference ;

(* Function calls (future) *)
function_call = function_name "(" [ argument_list ] ")" ;
argument_list = action_value { "," action_value } ;

(* Variable references (future) *)
variable_reference = "$" identifier | "${" field "}" ;

(* Terminals *)
field = identifier ;
operator = "contains" | "equals" | "matches" | "starts_with" | "ends_with" | 
          "not_contains" | "not_equals" | "not_matches" ;
value = quoted_string ;
logical_operator = "AND" | "OR" | "ALL" | "ANY" ;
function_name = identifier ;
quoted_string = "\"" string "\"" | "'" string "'" ;
identifier = letter { letter | digit | "_" } ;
string = { character } ;
```

## Assignment Operators

### `=` (Set Value)
Overwrites the field with the new value.

```
channel_name contains "sport" SET group_title = "Sports"
```

### `+=` (Append Value)
Appends to existing value (with space separator for strings).

```
channel_name contains "HD" SET channel_name += " [High Definition]"
```

### `?=` (Set If Empty)
Sets value only if the field is currently empty or null.

```
group_title equals "" SET group_title ?= "Uncategorized"
```

### `-=` (Remove Value)
Removes substring from existing value.

```
channel_name contains "[AD]" SET channel_name -= "[AD]"
```

## Field Types and Validation

### Stream Source Fields
```
channel_name    : string  - Channel display name
group_title     : string  - Channel group/category  
tvg_id          : string  - Electronic Program Guide ID
tvg_logo        : string  - Logo URL
tvg_name        : string  - Alternative channel name
tvg_shift       : string  - Time shift offset
stream_url      : string  - Stream URL (advanced use)
language        : string  - Channel language
category        : string  - Custom category field
```

### EPG Source Fields
```
program_title       : string - Program name
program_description : string - Program description
category           : string - Program category
language           : string - Program language
channel_id         : string - Channel identifier
channel_name       : string - Channel name
channel_logo       : string - Channel logo URL
channel_group      : string - Channel group
```

## Syntax Examples

### Basic Action Assignment
```
group_title equals "" SET group_title = "Uncategorized"
```

### Multiple Actions
```
channel_name contains "sport" SET group_title = "Sports", category = "entertainment"
```

### Complex Conditions with Actions
```
(channel_name contains "sport" OR channel_name contains "football") AND language equals "en" 
SET group_title = "English Sports", tvg_logo = "sports-logo.png"
```

### Different Assignment Operators
```
channel_name contains "HD" SET channel_name += " [HD Quality]"
tvg_logo equals "" SET tvg_logo ?= "default-logo.png"
channel_name contains "[AD]" SET channel_name -= "[AD]"
```

### Conditional Categorization
```
tvg_id starts_with "bbc" SET tvg_logo = "https://logos.example.com/bbc.png"
```

## Parsing Strategy

### Tokenization Enhancements
1. **Existing tokens**: field, operator, value, logical_operator, modifiers
2. **New tokens**: SET keyword, assignment operators (=, +=, ?=, -=), comma separator
3. **Token precedence**: SET keyword has lower precedence than logical operators

### Parser Modifications
1. **Condition parsing**: Unchanged - existing logic preserved
2. **Action parsing**: New parser branch triggered by SET keyword
3. **Validation**: Semantic validation for field types and operator compatibility

### AST Structure Extensions
```rust
pub enum ExtendedExpression {
    ConditionOnly(ConditionTree),
    ConditionWithActions {
        condition: ConditionTree,
        actions: Vec<Action>,
    },
}

pub struct Action {
    pub field: String,
    pub operator: ActionOperator,
    pub value: ActionValue,
}

pub enum ActionOperator {
    Set,        // =
    Append,     // +=
    SetIfEmpty, // ?=
    Remove,     // -=
}

pub enum ActionValue {
    Literal(String),
    Function(FunctionCall),    // Future
    Variable(VariableRef),     // Future
}
```

## Backward Compatibility

### Parsing Strategy
1. **Detection**: Check for SET keyword in expression
2. **Legacy mode**: If no SET keyword, use existing parser
3. **Extended mode**: If SET keyword present, use extended parser
4. **Fallback**: Invalid extended syntax falls back to condition-only mode

### Migration Path
1. **Phase 1**: Extended parser supports existing condition syntax
2. **Phase 2**: Gradual introduction of action syntax
3. **Phase 3**: Optional migration tools for converting form-based rules

## Validation Rules

### Syntax Validation
1. **Parentheses**: Balanced and properly nested
2. **Quotes**: Matched and properly escaped
3. **Keywords**: Proper case and spacing
4. **Operators**: Valid for field types

### Semantic Validation
1. **Field existence**: Field names must exist for source type
2. **Operator compatibility**: Operators must be valid for field types
3. **Value format**: Values must match field type constraints
4. **Action validity**: Assignment operators must be compatible with field types

### Runtime Validation
1. **Circular references**: Prevent infinite loops in actions
2. **Value constraints**: Respect field length and format limits
3. **Side effects**: Validate that actions don't break data integrity

## Error Handling

### Syntax Errors
```
"Unmatched quote at position 15"
"Missing closing parenthesis"
"Invalid assignment operator '>=' at position 25"
"Expected field name after SET keyword"
```

### Semantic Errors
```
"Unknown field 'invalid_field' for stream sources"
"Operator 'matches' requires string value, got number"
"Assignment operator '+=' not valid for field 'tvg_shift'"
"Circular reference detected in action chain"
```

### Recovery Strategies
1. **Partial parsing**: Parse valid portions, report specific errors
2. **Suggestion system**: Offer corrections for common mistakes
3. **Graceful degradation**: Fall back to condition-only mode when possible

## Performance Considerations

### Parsing Optimization
1. **Early termination**: Stop parsing at first unrecoverable error
2. **Lazy evaluation**: Only parse actions if condition matches
3. **Caching**: Cache compiled expressions for repeated use
4. **Batch processing**: Optimize for bulk data transformation

### Memory Management
1. **AST size**: Minimize memory footprint of parsed expressions
2. **String interning**: Reuse common field names and values
3. **Expression pooling**: Reuse expression objects when possible

## Future Extensions

### Advanced Features
1. **Function calls**: `upper()`, `trim()`, `if()`, `lookup()`
2. **Variable references**: `$channel_name`, `${group_title}`
3. **Conditional actions**: `if(condition, then_value, else_value)`
4. **External lookups**: `lookup_logo(tvg_id)`, `api_call(url)`

### Syntax Extensions
1. **Multiple SET clauses**: Sequential actions with dependencies
2. **Conditional SET**: `SET IF condition THEN actions`
3. **Batch operations**: `SET ALL matching_condition`
4. **Pipe operations**: `field | function | action`

This grammar provides a solid foundation for Phase 3 implementation while maintaining full backward compatibility with the existing filter system.