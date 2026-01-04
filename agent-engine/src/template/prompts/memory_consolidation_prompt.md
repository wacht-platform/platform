You are consolidating memories. Given a new fact and existing similar facts, determine the best outcome.

## Input
- **NEW FACT**: The information the agent wants to save
- **SIMILAR EXISTING FACTS**: Facts already in memory that are semantically related

## Your Task
Analyze whether the new fact should be:
1. **DUPLICATE** - The new fact is essentially the same as an existing one
2. **CONSOLIDATED** - The facts should be merged into one comprehensive statement

## Analysis Steps
1. Compare the new fact with each existing fact
2. Identify unique information in the new fact
3. Determine if any existing facts can be combined with the new one

## Quality Guidelines for Consolidated Statements
- **Preserve specificity**: Keep numbers, dates, names, technical details
- **Combine related info**: Merge related facts into one coherent statement
- **Remove redundancy**: Don't repeat the same information twice
- **Keep it concise**: One clear, comprehensive statement
- **Prefer detail over brevity**: When in doubt, keep the more specific version

## Output Format
You MUST respond with valid JSON only:

```json
{
  "decision": "duplicate|consolidate",
  "consolidated_content": "The merged statement (only if decision=consolidate)",
  "reason": "Brief explanation of your decision"
}
```

## Examples

### Example 1: Duplicate
NEW FACT: User prefers TypeScript
EXISTING: User prefers TypeScript for all projects
```json
{"decision": "duplicate", "consolidated_content": null, "reason": "Existing fact is more specific"}
```

### Example 2: Consolidate
NEW FACT: API timeout is 30 seconds
EXISTING: API uses retry logic with 3 attempts
```json
{"decision": "consolidate", "consolidated_content": "API uses retry logic with 3 attempts and a 30 second timeout", "reason": "Combined timeout with retry config"}
```

### Example 3: Consolidate Multiple
NEW FACT: User timezone is UTC
EXISTING: 
- User works 9am-5pm
- User is based in London
```json
{"decision": "consolidate", "consolidated_content": "User is based in London (UTC timezone), works 9am-5pm", "reason": "Merged location, timezone, and schedule"}
```

## Now Process

NEW FACT: {{new_fact}}

SIMILAR EXISTING FACTS:
{{#each existing_facts}}
- {{this}}
{{/each}}

Respond with JSON only:
