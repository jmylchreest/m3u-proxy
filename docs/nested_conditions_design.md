# Nested Conditions Design

## Goal
Support complex filter expressions like: `(A=B AND (C=D OR E=F) AND X=Y)`

## Current Structure
- `filters` table: single `logical_operator` for all conditions
- `filter_conditions` table: flat list of field/operator/value

## Proposed Tree Structure

### Database Schema Addition
Add to `filters` table:
```sql
ALTER TABLE filters ADD COLUMN condition_tree TEXT; -- JSON tree structure
```

### Tree Node Types
Each node can be either:

1. **Condition Node** (Leaf):
```json
{
  "type": "condition",
  "field": "channel_name", 
  "operator": "contains",
  "value": "news"
}
```

2. **Group Node** (Container):
```json
{
  "type": "group",
  "operator": "and",  // or "or"
  "children": [
    // ... child nodes (conditions or groups)
  ]
}
```

### Complete Example
Expression: `channel_name contains "news" AND (group_title equals "Sports" OR group_title equals "Movies")`

Tree representation:
```json
{
  "type": "group",
  "operator": "and",
  "children": [
    {
      "type": "condition",
      "field": "channel_name",
      "operator": "contains", 
      "value": "news"
    },
    {
      "type": "group", 
      "operator": "or",
      "children": [
        {
          "type": "condition",
          "field": "group_title",
          "operator": "equals",
          "value": "Sports"
        },
        {
          "type": "condition", 
          "field": "group_title",
          "operator": "equals",
          "value": "Movies"
        }
      ]
    }
  ]
}
```

## Implementation Strategy

### 1. Backward Compatibility
- Keep existing `logical_operator` and `filter_conditions` structure
- When `condition_tree` is NULL/empty, use legacy flat structure
- When `condition_tree` is present, use tree evaluation

### 2. Text Parser
Support user-friendly text input:
```
channel_name contains "news" AND (group_title equals "Sports" OR group_title equals "Movies")
```

Parse to tree structure with proper operator precedence.

### 3. Evaluation Engine
Recursive evaluation:
```rust
fn evaluate_node(node: &ConditionNode, channel: &Channel) -> bool {
    match node.node_type {
        ConditionNodeType::Condition => evaluate_condition(&node.condition, channel),
        ConditionNodeType::Group => {
            let results: Vec<bool> = node.children.iter()
                .map(|child| evaluate_node(child, channel))
                .collect();
            
            match node.operator {
                LogicalOperator::And => results.iter().all(|&x| x),
                LogicalOperator::Or => results.iter().any(|&x| x),
            }
        }
    }
}
```

### 4. UI Enhancement
- Support text input with parentheses
- Provide syntax highlighting
- Show visual tree representation (optional)
- Validate parentheses balancing

## Migration Strategy

1. Add `condition_tree` column to filters table
2. Existing filters continue to work with flat structure
3. New filters can use either flat or tree structure
4. Gradual migration of complex filters to tree format

## Benefits

1. **Powerful Expressions**: Support any complexity of nested conditions
2. **Backward Compatible**: Existing filters continue to work unchanged  
3. **User Friendly**: Natural text input with parentheses
4. **Performant**: Tree evaluation is efficient
5. **Extensible**: Easy to add new operators or node types

## Text Expression Grammar

```
expression := group | condition
group := '(' expression (operator expression)* ')'
condition := field operator value
operator := 'AND' | 'OR'
field := 'channel_name' | 'group_title' | 'tvg_id' | 'tvg_name' | 'stream_url'
```

Examples:
- `channel_name contains "sport"`
- `channel_name contains "sport" AND group_title equals "HD"`  
- `(channel_name contains "sport" OR channel_name contains "news") AND group_title equals "HD"`
- `channel_name contains "sport" AND (group_title equals "HD" OR group_title equals "4K")`