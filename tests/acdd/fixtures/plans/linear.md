---
title: "Canonical Linear Plan"
doc_type: plan
purpose: "Test fixture for linear two-step in-place plan execution"
depends_on: []
---

# Canonical Linear Plan

## 1. Step 1: First Action

- **Target**: Implement first action
- **Scope**: src/first.rs
- **Status**: ready

## 2. Step 2: Second Action

- **Target**: Implement second action
- **Scope**: src/second.rs
- **Depends On**: ["Step 1: First Action"]
- **Status**: pending
