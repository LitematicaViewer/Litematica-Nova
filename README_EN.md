[简体中文](./README.md) | **English**

# Litematica Blueprint Assistant

### Minecraft tool - make schematic viewing in Minecraft lighter and more convenient

[![GitHub Downloads (all assets, all releases)](https://img.shields.io/github/v/release/Albertchen857/LitematicaViewer)]()
[![三连](https://img.shields.io/badge/-一键三连-00A1D6?logo=bilibili&logoColor=white)](https://www.bilibili.com/video/BV1H9ZVYtEta/?spm_id_from=333.1387.homepage.video_card.click&vd_source=20c164cb28b2da114329d8728dad750f)
[![转发](https://img.shields.io/badge/-转发-00A1D6?logo=bilibili&logoColor=white)](https://space.bilibili.com/3494373232741268)
[![YoutubeIntro](https://img.shields.io/badge/-Youtube-00A1D6?logo=youtube&logoColor=red)](https://www.youtube.com/watch?v=0nofWrfKJeg)



Source project GitHub link: https://github.com/albertchen857/LitematicaViewer
Download stable EXE files from the Resources panel on the right
Clone the latest (preview) Python version by yourself (if you find issues, please open an issue, thanks)
Please give this project a star. I want stars~ target: 100 stars

A lightweight and convenient schematic viewer

## Feature List
Implemented and planned features are listed here. For full design details, please check [this document](docs/design.md)

### Schematic Library
Schematic resource manager that collects scattered files and manages them in one place
- [ ] Basic implementation
- [ ] Fetch schematics from external sources
### Properties
Read and parse SNBT data inside files, and provide suitable editing options
- [x] Import schematic files
- [x] Parse and read SNBT data
	- [x] Text data (read/write): internal name, author, description
	- [x] Format data (read-only): time, size, volume, version
	- [x] Image data (read/write): preview image (array)
	- [x] Chunk data (read-only): regions
- [ ] Version conversion

### Statistics
Run statistical analysis based on schematic content to quickly evaluate schematic attributes
- [x] Statistical metrics from the source project
	- [x] Skewness: redstone, liquids
	- [x] Statistical categorization
	- [x] Density
- [ ] Block category statistics
- [ ] Survival mode practical indicators
	- [ ] Scarce material requirement tags: e.g., slime blocks, quartz, coral
	- [ ] Mechanic tags: e.g., TNT duplication

### Layer View
Customize Y-level planar display
- [ ] Block state overlay display
- [ ] Block entity detail panel
	- [ ] Container content analysis
	- [ ] Sign content analysis

### Rendering
Render schematic structures in 3D
- [ ] Embedded plugin rendering ([reference project](https://github.com/misode/vscode-nbt))
	- [x] Basic implementation
	- [ ] Region splitting
	- [x] Hot reload and replace game assets
- [ ] Self-built deepslate rendering
	- [x] Basic implementation
	- [x] Region splitting
	- [ ] Hot reload and replace game assets
- [x] Preset camera directions
- [x] Custom field of view (perspective/orthographic)
- [x] Capture and export images
- [x] Render full in-frame model and export image
	- [x] Calculate centroid: camera distance

### Replacement
Quickly replace/restrict different blocks in the schematic
- [ ] 
- [ ] 

### Material List
List block counts included in selected regions or layers
- [x] Unit statistics (shulker boxes/stacks/remainders)
- [x] Table item hover panel
- [x] Multi-workbook coexistence
- [x] Export list data
	- [x] `.csv` comma-separated value file
	- [x] `.txt` ASCII art table
- [x] Import and parse list data
	- [x] Read `.csv` comma-separated value files exported by the Litematica mod
- [x] Minecraft block item icons

### User Interface
- [ ] Themes
	- [x] Metro10: a theme similar to the Windows 10 interface
	- [x] Minecraft: a theme similar to the Minecraft interface
	- [ ] Bulletin Azure: a theme similar to the Blue Archive interface
		- [ ] "Bulletin" means briefing/archive; "Azure" means azure blue
	- [ ] ...

### Enumerator
Provide or customize preset filter enumerations
- [ ] Retrieve full table of in-game data values
- [ ] Drag-and-drop categorization

### Simple Geometry Schematic Generator
Quickly generate specified schematic structures
- [ ] Regular cube
- [ ] Sphere
- [ ] Custom curve/surface equations

