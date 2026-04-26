# R-Clash Project Mandates & Design Philosophy

This document serves as the foundational mandate for the R-Clash project. All AI agents and developers must adhere to these principles.

## 1. Core Design Philosophy: Dual-Path UX
The interface must cater to two distinct user segments simultaneously:
- **Beginner-Friendly (Conversational UI)**: Abstract complex technical logic into natural language. 
  - *Example*: Use "I want to [Keep] nodes where name [Contains] [Hong Kong]" instead of raw regex input.
  - Use "building blocks" and "presets" (e.g., "Only HK nodes", "Clean garbage nodes") for common tasks.
- **Expert-Empowered (Raw Access)**: Provide "backdoors" for power users.
  - Enable "Regex Mode" or "Raw Scripting" via toggles without cluttering the basic UI.
  - Terminology should be action-oriented (e.g., "Cleaning Scheme" instead of "Filter Logic").

## 2. Technical Principles: Smart Merge & Namespace Isolation
- **Unified Configuration**: The system operates on a single global node pool aggregated from multiple sources.
- **Namespace Strategy**: Every node must follow the `Name@Subscription` naming convention to ensure uniqueness and source transparency.
- **Resource Providers**: Subscriptions are treated purely as "Resource Providers". They feed nodes into the system after passing through a user-defined "Cleaning Scheme".

## 3. Visual Language: High-Signal & Physicality
- **Emotional Design**: Use color coding for immediate psychological feedback:
  - **Green**: Pass, Keep, Success, Connected.
  - **Red**: Block, Discard, Error, Disconnected.
  - **Amber/Yellow**: Warning, High Latency, Testing.
- **Aesthetics**: Follow "Glassmorphism" and "Bento Grid" styles. 
  - Use large border radii (up to `2.5rem`), deep shadows, and backdrop blurs to create a sense of depth and tactile physicality.
- **Feedback**: Every interaction must provide immediate visual feedback (Toasts, loading spinners, state changes).

## 4. Engineering Standards
- **API First**: All frontend interactions must be documented in `doc/openapi.yaml` and implemented via standard `fetch` calls.
- **Stateful Mocking**: During development, use MSW (Mock Service Worker) with in-memory state persistence to simulate real backend behavior.
- **Type Safety**: Maintain strict TypeScript definitions. Avoid `any` where possible.
