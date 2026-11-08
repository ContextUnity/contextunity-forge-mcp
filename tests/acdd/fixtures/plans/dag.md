---
title: "Canonical DAG Plan"
doc_type: plan
purpose: "Test fixture for branching DAG in-place plan execution"
depends_on: []
---

# Canonical DAG Plan

## 1. Step A: Independent Core

- **Target**: Core changes
- **Scope**: src/core.rs
- **Status**: ready

## 2. Step B: Extension Beta

- **Target**: Extension on core
- **Scope**: src/beta.rs
- **Depends On**: ["Step A: Independent Core"]
- **Status**: pending

## 3. Step C: Extension Gamma

- **Target**: Alternative extension on core
- **Scope**: src/gamma.rs
- **Depends On**: ["Step A: Independent Core"]
- **Status**: pending
