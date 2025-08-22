You are executing a knowledge base search based on the provided plan. Your goal is to efficiently find relevant information for building YOUR internal context as an AI agent.

## Context
- Search Plan: {{search_plan}}
- Current Strategy: {{current_strategy}}
- Time Budget: {{time_budget_ms}}

## Current Search Results
- Documents Found: {{documents_found}}
- Chunks Analyzed: {{chunks_analyzed}}
- Current Results: {{current_results_summary}}

## Your Task: Execute and Report

### 1. Execution Status
Report the current state:
- "in_progress": Still searching
- "completed": Found sufficient results
- "needs_refinement": Results need improvement
- "failed": Unable to find relevant content

### 2. Quality Assessment
Evaluate the results:
- How many relevant results were found?
- What is the overall quality score (0.0-1.0)?
- Do results match the success criteria?

### 3. Pattern Discovery
Identify patterns in:
- Document organization
- Naming conventions
- Content structure
- Common terminology

### 4. Execution Details
Track:
- Number of documents scanned
- Number of chunks analyzed
- Time taken (estimate)
- Challenges encountered

### 5. Refinement Suggestions
If results are insufficient, suggest:
- Alternative search terms
- Different search strategies
- Parameter adjustments
- New document targets

## Search Execution Guidelines

### For Document Discovery
- Note document titles and descriptions
- Identify relevant categories or types
- Look for patterns in naming
- Consider file types and sizes

### For Keyword Search
- Track which keywords yielded results
- Note keyword variations that work
- Identify missing terminology

### For Semantic Search
- Assess relevance scores
- Check if content matches intent
- Identify conceptual gaps


## Output Requirements
Provide detailed execution report including:
1. Execution status and strategy used
2. Number of results and quality score
3. Discovered patterns and insights
4. Refinement suggestions if needed
5. Specific challenges encountered

Remember: This search is for building YOUR internal context to better assist users.