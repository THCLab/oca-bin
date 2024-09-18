# Changelog

All notable changes to this project will be documented in this file.

## [0.4.6-rc.3] - 2024-09-18

### 🚀 Features

- Allow validate multiple files

### 🐛 Bug Fixes

- Save full path in graph nodes
- Allow both arguments in validate command

### 🚜 Refactor

- Improve formatting of build error display
- Reformat and remove unused code

## [0.4.6-rc.2] - 2024-09-12

### 🚀 Features

- Allow building multiple ocafiles

### 🐛 Bug Fixes

- Allow both arguments in build command
- Avoid building the same ocafile twice

### ⚙️ Miscellaneous Tasks

- Release 0.4.6-rc.2 version

## [0.4.6-rc.1] - 2024-09-12

### 🚀 Features

- Add details window
- Add dependent files to details
- Add deps command
- Cache processed refn during build
- Cache processed refn during validate

### 🐛 Bug Fixes

- Fix loading repository url from config file
- Better error messages
- Catch unexpected errors

### ⚙️ Miscellaneous Tasks

- Release 0.4.6-rc.1 version

## [0.4.5] - 2024-08-28

### 🐛 Bug Fixes

- *(tui)* Notify user of missing config fields
- *(tui)* Notify user about any error in tui

### ⚙️ Miscellaneous Tasks

- Fix clippy warnings
- Release 0.4.5 version

## [0.4.4] - 2024-08-23

### 🚀 Features

- Add handling for BundleElement::Transformation to build transformation files

### ⚙️ Miscellaneous Tasks

- Release 0.4.4 version

## [0.4.3] - 2024-08-20

### 🐛 Bug Fixes

- Correct failing tests

### ⚙️ Miscellaneous Tasks

- Cleanup README.md
- Update README.md
- Update said package to 0.4.1 and oca packages to 0.5.4
- Release 0.4.3 version

## [0.4.2] - 2024-07-16

### 🐛 Bug Fixes

- Use repository_url from config file in publish
- Remove sign command
- Don't panic when refn is missing in ocafile
- Don't panic on not supported chars in refn

### ⚙️ Miscellaneous Tasks

- Release 0.4.2 version

## [0.4.1] - 2024-07-08

### 🚀 Features

- Add help window
- Make changes list scrollable

### 🐛 Bug Fixes

- Remove unwrap
- Don't close tui when not supported button pressed
- *(tui)* Prevent exit on window resize
- *(publish)* Show refn of published element
- Change default publishing timeout
- Compute presentation digest during generation
- Ensure all files have refn set
- *(tui)* Unselect elements that were built
- *(tui)* Unselect elements that were published

### ⚙️ Miscellaneous Tasks

- Release 0.4.0 version
- Rewamp CHANGELOG
- Release 0.4.1 version

### Build

- Use git cliff to auto-update CHANGELOG.md

## [0.4.0-rc.13] - 2024-06-17

### 🚀 Features

- Allow setting publishing timeout in tui
- Show deleted files in changes

### 🚜 Refactor

- Remove unwraps

### ⚙️ Miscellaneous Tasks

- Bump oca-presentation version
- Release 0.4.0-rc.13 version
- Changelog for rc13

### Fit

- Change mapping format

## [0.4.0-rc.12] - 2024-06-12

### 🚀 Features

- *(tui)* Add publish command
- Track changes with git2 crate
- Add changes tree
- *(tui)* Commit only selected changes
- *(tui)* Show tree of dependent changes
- Add mapping function
- Process arrays in mapping function
- Add mapping command

### 🐛 Bug Fixes

- Build initial ocafile
- Return error on duplicate refn tag

### 🚜 Refactor

- Stop using git for changes tracking
- Cargo fmt

### ⚙️ Miscellaneous Tasks

- Release 0.4.0-rc.12 version
- Add changelog for rc12

## [0.4.0-rc.11] - 2024-05-08

### 🚀 Features

- Tui draft
- Use petgraph crate
- Add expandlable list
- Enable expanding references
- Show only chosen directroy
- Handle Enter and PageUp/PageDown buttons
- Add validate directory command
- *(tui)* Add errors window
- *(tui)* Allow switching windows
- Validate only selected form
- Add build command to tui
- Update status comments
- Render messages while building
- Show building errors
- Add timeout for publish command
- Validate all files before building
- Build only selected ocafile
- *(tui)* Select multiple elements
- Validate or build multiple files
- Select/unselect all
- Allow error selecting
- Allow multiple elements validation and building

### 🐛 Bug Fixes

- Minor changes
- Split main into modules
- Add DependencyGraph struct
- Remove unused code
- Change reference color
- Use ocafiles from subdirectories
- Update array type display
- Allow building from ocafile
- Remove local dependencies
- Check if directory exists
- Ignore files without refn
- Render missing refn error
- *(tui)* Print relative path in errors
- Don't block on validation
- Render errors as a list
- Update name if it changed
- Update oca dependencies
- Add commands description
- Show validation result
- Update footer
- Show last message in output window
- Fix ocafile build command
- Setup panic hooks
- Setup logging
- Remove todo
- Print help if no command
- *(tui)* Update footer

### 🚜 Refactor

- Fix clippy warnings
- Add GraphError
- Cargo fmt
- Rename error to message
- Remove unused code
- Reformat and cargo clippy

### ⚙️ Miscellaneous Tasks

- Update presentation dependency
- Improve naming in graph logic
- Add info logs for publish
- Remove path from oca-presentation deps
- Publish dependency only once
- Update cargo lock
- Fix clippy warnings and reformat
- Update dependencies
- Release 0.4.0-rc.11 version

## [0.4.0-rc.10] - 2024-03-08

### 🚀 Features

- Fix number type in presentation

### ⚙️ Miscellaneous Tasks

- Add changelog for rc.10
- Release 0.4.0-rc.10 version

## [0.4.0-rc.9] - 2024-03-07

### 🚀 Features

- Support number in pres type

### ⚙️ Miscellaneous Tasks

- Default reduce verbosity
- Add changelog for rc.9
- Release 0.4.0-rc.9 version

## [0.4.0-rc.8] - 2024-03-07

### ⚙️ Miscellaneous Tasks

- Update presentation crate
- Release 0.4.0-rc.8 version

## [0.4.0-rc.7] - 2024-02-26

### 🚀 Features

- Handle dependecies while building object

### 🐛 Bug Fixes

- Update oca-presentation version

### ⚙️ Miscellaneous Tasks

- Release 0.4.0-rc.7 version

## [0.4.0-rc.6] - 2024-02-02

### 🚀 Features

- Add presentation field

### 🐛 Bug Fixes

- Remove attributes translations from po field
- Change extention according to provided format

### ⚙️ Miscellaneous Tasks

- Release 0.4.0-rc.6 version

## [0.4.0-rc.5] - 2024-01-29

### 🚀 Features

- Fail presentation parsing in said is wrong
- Convert OcaBundle to Presentation
- Support array of refs in presentation
- Load languages from overlays
- Allow presentation in yaml format
- Generate translations
- Fill interaction section

### 🐛 Bug Fixes

- Update oca-presentation dependency
- Warnings
- Presentation test
- Remove presentation get subcommand
- List of oca objects in local repo
- Replace parse command with valdiate
- Add namespaces in `i` section
- Generate namespaces for arrays

### 🚜 Refactor

- Update presentation command
- Add subcommands for presentation
- Add CliError
- Reformat

### 📚 Documentation

- Add description of presentation subcommands

### ⚙️ Miscellaneous Tasks

- Update cargo.toml
- Release 0.4.0-rc.5 version

### Fir

- Reformat

## [0.4.0-rc.4] - 2024-01-09

### 🚀 Features

- Build ocafiles from directory
- Build from dir with recursive

### ⚙️ Miscellaneous Tasks

- Release 0.4.0-rc.3 version
- Release 0.4.0-rc.3 version
- Remove unused code
- Release 0.4.0-rc.4 version

## [0.4.0-rc.2] - 2024-01-09

### ⚙️ Miscellaneous Tasks

- Add release configuration
- Exclude gitattributes
- Fix version in cargo
- Release 0.4.0-rc.2 version

### Release

- 0.4.0-rc.2

## [0.4.0-rc.1] - 2024-01-09

### 🚀 Features

- Add possibility to display ast for given said
- Bump oca-rs to 0.3.7
- Support dereference of refn
- Support for pages in list
- Always dereference local references for oca bundle
- Add with dependency flag for fetching all bundles at once
- Publish all dependencies at once
- Show refn for build if available
- Add support for oca presentation

### 🐛 Bug Fixes

- Handle properly remote repo configuration
- Publish endpoint

### ⚙️ Miscellaneous Tasks

- Adopt to changes from string to SAID in oca-rs

## [0.3.0] - 2023-11-15

### 🚀 Features

- List references for each available object

### 🐛 Bug Fixes

- Allow to control log level

### ⚙️ Miscellaneous Tasks

- Update dependencies
- Update documentation on subcomands
- Remove dep path to compile project on github action

## [0.2.0] - 2023-11-03

### 🚀 Features

- Add config subcommand
- Add publish command
- Implement configuration

## [0.1.0] - 2023-10-17

### 🐛 Bug Fixes

- Bundled sql dependency to pass builds on windows

<!-- generated by git-cliff -->
