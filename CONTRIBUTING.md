# Contributing Guidelines

Thank you for considering contributing to our project! Please follow these guidelines to ensure smooth collaboration.

## General Guidelines

### Forking the Repository

1. Fork the repository.
2. Clone the forked repository to your local machine.
3. Implement your changes.
4. Push your changes to your forked repository.
5. Create a pull request to the main repository.
6. Wait for the pull request to be reviewed and merged by the repository owner.

## Versioning Specifications and Styles

### GitHub Issues

1. Create GitHub issues for each task.
2. Please add detailed descriptions to the issues.

### Feature Branches

1. Create feature branches for each task from Trello.
2. Branch naming convention:
   - The branch name should start with the GitHub issue number.
   - After the issue number, use the issue title in lowercase separated with "-" symbol.
   - Example: `13-login-system` for GitHub issue #13 with the title "Login system".
   - [Optional things to consider](https://tilburgsciencehub.com/building-blocks/collaborate-and-share-your-work/use-github/naming-git-branches/)

### Commits

1. Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for all commits.
2. Format: `<type>[optional scope]: <description>`
3. Types:
   - `fix`: Bugfixes, code fixes.
   - `feat`: Feature implementation.
   - `chore`: Code cleanup.
4. Example: `feat(api)!: send an email to the customer when a product is shipped`
   - The "!" flag is optional and indicates breaking changes.

### Pull Requests

1. Submit your changes via pull requests.
2. Reference the relevant GitHub issue in your pull request.
3. Provide a clear and concise description of your changes.
   **NOTE**: The pull request should only be merged after reviewing it by (at least) another team member. If there are no related review comments, then the reviewer should merge the PR. This review step ensures the quality and consistency of the codebase.

## Variable Naming Conventions

Follow NextJS and React TypeScript naming conventions as much as possible. Resource: [https://medium.com/@mirazhs/naming-conventions-in-next-js-boosting-seo-and-code-maintainability-d4150fe7e6e](https://medium.com/@mirazhs/naming-conventions-in-next-js-boosting-seo-and-code-maintainability-d4150fe7e6e)
