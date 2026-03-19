# Godot 4.x Editor - Comprehensive Feature Reference

> Target comparison document for Patina web-based editor clone.
> Compiled from official Godot docs + community sources, March 2026.

---

## 1. Scene Tree Panel (Scene Dock)

### Node Operations
- **Add child node** - button at top of dock; opens "Create New Node" dialog
- **Quick-add root nodes** - buttons for "2D Scene", "3D Scene", "User Interface", "Other Node"
- **Delete node** - Delete key or right-click context menu
- **Rename node** - double-click node name in tree (F2 shortcut)
- **Duplicate node** - Ctrl+D duplicates selected node(s) with children
- **Reparent node** - drag-drop within the tree to move under new parent
- **Reorder siblings** - drag-drop to change order among siblings
- **Copy/Paste nodes** - Ctrl+C / Ctrl+V across scenes
- **Cut node** - Ctrl+X
- **Multi-select** - Shift+click for range, Ctrl+click for toggle

### Visual Indicators
- **Node type icons** - color-coded by category (blue=Node2D, green=Control, etc.)
- **Visibility toggle** - eye icon per node (shows/hides in viewport)
- **Lock toggle** - lock icon per node (prevents selection in viewport)
- **Scene instance indicator** - film-strip icon for instanced scenes; click to open
- **Script indicator** - scroll icon when a script is attached
- **Group indicator** - icon when node belongs to groups
- **Signal connection indicator** - icon when signals are connected
- **Unique name indicator** - % prefix for scene-unique nodes
- **Warning icon** - yellow triangle when node has configuration issues

### Hierarchy Features
- **Expand/collapse** - arrow toggles to show/hide children
- **Search/filter** - text field filters visible nodes by name
- **Scene instancing** - drag .tscn from FileSystem dock into tree
- **Make scene from branch** - right-click to save subtree as new .tscn
- **Editable children** - toggle for instanced scenes to allow editing children
- **Merge from scene** - import nodes from another scene file

### Context Menu (Right-Click)
- Add Child Node / Add Sibling Node
- Cut / Copy / Paste / Duplicate
- Rename
- Move Up / Move Down (reorder)
- Change Type (convert node to different type)
- Attach Script / Detach Script
- Make Scene Root
- Save Branch as Scene
- Merge From Scene
- Copy Node Path
- Delete Node(s)
- Access Node Configuration Warning
- Toggle Visibility / Lock
- Manage Groups

### Groups
- Add node to named groups via Node dock
- Remove from groups
- Group indicator in tree
- Select all nodes in a group
- Groups are scene-level or global

---

## 2. Inspector Panel (Inspector Dock)

### Top Toolbar (Left to Right)
- **New Resource Window** - create and edit a resource in memory
- **Open Resource** - load a resource from FileSystem
- **Save Resource** - persist edited resource to disk
- **Resource Menu** dropdown:
  - Edit Resource from Clipboard
  - Copy Resource
  - Show in FileSystem
  - Make Resource Built-In (convert to embedded)
- **Back/Forward navigation** - "<" and ">" traverse editing history
- **History List** - dropdown showing all recently edited objects

### Node Header
- Selected node icon + name
- Documentation quick-link button (?)
- Click node name to list available sub-resources

### Search & Filter
- Case-insensitive search bar
- Real-time letter-by-letter filtering
- Example: typing "vsb" finds "Visibility" property

### Property Organization
- Properties grouped by **class** (Node, Node2D, Sprite2D, etc.)
- Each class contains expandable **sections** (Transform, Texture, etc.)
- Right-click any class header to open documentation

### Property Types & Editors

| Type | Editor Widget |
|------|--------------|
| bool | Checkbox |
| int | Spin box with arrows, or slider |
| float | Spin box, slider, or drag-to-adjust |
| String | Text field |
| StringName | Text field with type indicator |
| enum | Dropdown menu |
| flags/bitmask | Grid of checkboxes |
| Vector2 | Two float fields (x, y) |
| Vector2i | Two int fields (x, y) |
| Vector3 | Three float fields (x, y, z) |
| Vector3i | Three int fields (x, y, z) |
| Vector4 | Four float fields |
| Rect2 | Four float fields (position + size) |
| Transform2D | 3x2 matrix fields |
| Transform3D | 4x3 matrix fields |
| Color | Color swatch + picker dialog (RGB/HSV, hex, alpha) |
| NodePath | Path selector with node picker button |
| Resource | Dropdown to create/load/edit; drag from FileSystem |
| Array | Expandable list with add/remove/reorder |
| Dictionary | Expandable key-value pairs |
| PackedByteArray | Specialized array editor |
| PackedStringArray | String list editor |
| PackedVector2Array | Vector2 list editor |
| PackedColorArray | Color list editor |
| Curve/Curve2D | Curve editor widget |
| Gradient | Gradient editor with color stops |
| AABB | Six float fields |
| Basis | 3x3 matrix fields |
| Plane | Four float fields (normal + distance) |
| Quaternion | Four float fields |
| Projection | 4x4 matrix fields |
| RID | Read-only display |
| Callable | Read-only display |
| Signal | Read-only display |

### Property Interaction
- **Click to edit** text/number fields
- **Drag to adjust** numeric values (hold LMB and drag)
- **Slider** for ranged numeric properties
- **Revert icon** (undo arrow) appears on modified properties; click to restore default
- **Chain icon** for linked values (e.g., uniform scale); click to unlink
- **Hover** shows property description and script-callable name

### Context Menu (Right-Click on Property)
- Copy Property Value
- Paste Property Value
- Copy Property Path (for use in code)
- Favorite Property (pins to top for all objects of that type)
- Open Property Documentation

### Tools Menu (gear icon next to search)
- Expand All
- Collapse All
- Expand Non-Default (only sections with modified values)
- Property Name Style: Raw / Capitalized / Localized
- Copy Properties (all properties with values)
- Paste Properties
- Make Sub-Resources Unique

### Advanced Features
- **Favorited properties** pinned at inspector top for all objects of that class
- **Sub-resource inline editing** - click loaded sub-resource to inspect in-place
- **Exported script variables** (@export) appear alongside built-in properties
- **@export_group / @export_subgroup** for organizing custom properties
- **@export_category** for top-level grouping
- **@export hints** - range, enum, file path, multiline, etc.

---

## 3. 2D Viewport (Canvas Editor)

### Toolbar (Top of Viewport)
- **Select Mode (Q)** - click to select nodes; drag rectangle for multi-select
- **Move Mode (W)** - drag to translate selected nodes
- **Rotate Mode (E)** - drag to rotate selected nodes
- **Scale Mode (S)** - drag to scale selected nodes
- **Ruler Mode (R)** - measure distances in the viewport
- **Pan Mode (H)** - drag to pan the viewport
- **Pivot button** - set custom rotation pivot point

#### Modifier Keys in Select Mode
- Alt + drag: Move the selected node
- Ctrl + drag: Rotate the selected node
- Alt + Ctrl + drag: Scale the selected node
- Shift + click: Add/remove from selection

#### Snap Options (toolbar toggles)
- **Use Grid Snap** - snap movement to grid
- **Use Smart Snap** - snap to guides and other nodes
- **Configure Snap** dialog:
  - Grid step (X, Y)
  - Grid offset (X, Y)
  - Rotation step (degrees)
  - Scale step

#### View Options
- **Show Grid** toggle
- **Show Rulers** toggle
- **Show Guides** toggle
- **Show Origin** toggle
- **Show Viewport** toggle (game resolution outline)
- **Show Navigation** toggle
- **Show Y-Sort** toggle
- **Center Selection** button
- **Frame Selection** button

### Viewport Controls
- **Zoom** - mouse scroll wheel; Ctrl + scroll for fine zoom
- **Pan** - middle mouse button drag; Space + LMB drag
- **Zoom to fit** - shortcut to frame entire scene
- **Zoom percentage** - displayed in corner; click to reset

### Selection Features
- **Click to select** nodes in viewport
- **Rectangle select** - drag to select multiple nodes
- **Show list of selectable nodes** - when overlapping, right-click shows context menu of all nodes at position
- **Select locked nodes** override via menu
- **Select through** instanced scenes

### Transform Gizmos
- **Move gizmo** - X/Y axis arrows + free move center square
- **Rotate gizmo** - circular handle around selection
- **Scale gizmo** - axis handles + uniform scale center
- **Transform origin marker** - draggable pivot point
- **Local vs Global** coordinate toggle

### Canvas Features
- **Canvas layers** with independent transforms
- **CanvasModulate** for tinting
- **Origin cross** at (0,0)
- **Grid lines** configurable spacing/color
- **Rulers** along top and left edges
- **Guides** - draggable lines from rulers
- **Bone visualization** for Skeleton2D
- **Polygon editing** mode for polygon nodes
- **Path editing** mode for Path2D/Line2D

---

## 4. Toolbar (Main Editor Top Bar)

### Left Section - Scene Tabs
- Open scene tabs with close buttons
- Tab reordering via drag
- Middle-click to close tab
- Tab context menu (Close, Close Others, Close All, Show in FileSystem)
- "+" button to create new scene
- Asterisk (*) on tab name for unsaved changes

### Center Section - Run Controls
- **Play Scene (F5)** - run current project main scene
- **Pause (F7)** - pause running scene
- **Stop (F8)** - stop running scene
- **Play Current Scene (F6)** - run currently edited scene
- **Play Custom Scene** - select specific scene to run
- **Debug options** dropdown:
  - Deploy with Remote Debug
  - Small Deploy with Network Filesystem
  - Visible Collision Shapes
  - Visible Paths
  - Visible Navigation
  - Synchronize Scene Changes (live editing)
  - Synchronize Script Changes (live reload)

### Right Section - Editor Modes
- **2D** button - switch to 2D workspace
- **3D** button - switch to 3D workspace
- **Script** button - switch to script editor
- **Game** button - switch to game view (4.0+)
- **AssetLib** button - open asset library

### Menus
#### Scene Menu
- New Scene
- Open Scene / Open Recent
- Save Scene / Save Scene As / Save All Scenes
- Quick Open Scene (Ctrl+Shift+O)
- Revert Scene
- Close Tab
- Run Settings
- Reload Saved Scene

#### Project Menu
- Project Settings
- Version Control (VCS integration)
- Export
- Install Android Build Template
- Open User Data Folder
- Quit

#### Debug Menu
- Step Into / Step Over / Break / Continue
- Keep Debugger Open
- Debug with External Editor
- Customize Run Instances

#### Editor Menu
- Editor Settings
- Editor Layout (Save/Load/Delete)
- Manage Editor Features (profiles)
- Manage Export Templates
- Take Screenshot

#### Help Menu
- Search Help (F1)
- Online Documentation
- Q&A
- Report a Bug
- Copy System Info
- Toggle System Console
- About Godot

### Edit Menu (Global)
- Undo (Ctrl+Z)
- Redo (Ctrl+Shift+Z / Ctrl+Y)

---

## 5. Node Types for 2D (Create New Node Dialog)

### Dialog Features
- Searchable list of all node types
- Class hierarchy tree view
- Node description panel at bottom
- Favorites section for frequently used nodes
- Recent nodes section

### Core Nodes
- **Node** - base of all nodes
- **Node2D** - base for all 2D nodes (position, rotation, scale, z_index)
- **CanvasItem** - base for 2D rendering (modulate, self_modulate, visible)
- **CanvasLayer** - independent 2D rendering layer

### Sprites & Graphics
- **Sprite2D** - displays a texture
- **AnimatedSprite2D** - sprite with SpriteFrames animation
- **Polygon2D** - renders a 2D polygon with texture
- **Line2D** - 2D polyline with width/texture
- **MeshInstance2D** - 2D mesh rendering
- **MultiMeshInstance2D** - instanced 2D meshes
- **Parallax2D** - parallax scrolling (replaces ParallaxBackground+ParallaxLayer)
- **BackBufferCopy** - copies screen region for shader effects
- **CanvasModulate** - tints entire canvas

### Physics Bodies
- **CharacterBody2D** - kinematic body for player/NPC movement
- **RigidBody2D** - physics-simulated body
- **StaticBody2D** - immovable physics body
- **AnimatableBody2D** - physics body that can be moved by code

### Physics Collision
- **Area2D** - detects overlap/entry/exit of bodies
- **CollisionShape2D** - defines collision shape (child of body/area)
- **CollisionPolygon2D** - polygon-based collision shape
- **CollisionObject2D** - base class for collision objects

### Collision Shapes (Resources, not nodes)
- RectangleShape2D
- CircleShape2D
- CapsuleShape2D
- ConvexPolygonShape2D
- ConcavePolygonShape2D
- SegmentShape2D
- SeparationRayShape2D
- WorldBoundaryShape2D

### Physics Joints
- **PinJoint2D** - pin/pivot joint
- **GrooveJoint2D** - groove/slider joint
- **DampedSpringJoint2D** - spring joint

### Camera & Viewport
- **Camera2D** - 2D camera with smoothing, limits, drag margins, zoom
- **SubViewport** - off-screen rendering target
- **SubViewportContainer** - displays SubViewport content

### Lights & Shadows
- **PointLight2D** - 2D point/spot light
- **DirectionalLight2D** - 2D directional light
- **LightOccluder2D** - casts 2D shadows
- **CanvasModulate** - global color tinting

### TileMap
- **TileMapLayer** - tile-based 2D map layer (replaced monolithic TileMap in 4.3+)
- TileSet resource (atlas-based, with collision, navigation, custom data)
- TileMap editor: paint, line, rect, bucket fill, eraser, picker
- Terrain autotiling with matching rules
- Scatter (random placement)
- Pattern saving/loading

### Path & Navigation
- **Path2D** - defines a 2D path curve
- **PathFollow2D** - follows a Path2D
- **NavigationRegion2D** - defines navigable area
- **NavigationAgent2D** - pathfinding agent
- **NavigationObstacle2D** - dynamic obstacle for avoidance
- **NavigationLink2D** - connects separate navigation regions

### Audio
- **AudioStreamPlayer2D** - positional 2D audio
- **AudioStreamPlayer** - non-positional audio
- **AudioListener2D** - overrides audio listener position

### Particles
- **GPUParticles2D** - GPU-accelerated 2D particles
- **CPUParticles2D** - CPU-based 2D particles (more compatible)

### Skeleton & Animation
- **Skeleton2D** - 2D skeletal deformation
- **Bone2D** - individual bone in skeleton
- **PhysicalBone2D** - physics-driven bone
- **AnimationPlayer** - keyframe animation player
- **AnimationTree** - state machine / blend tree for animations
- **AnimatedSprite2D** - frame-by-frame sprite animation

### Raycasting
- **RayCast2D** - 2D raycast for collision detection
- **ShapeCast2D** - shape sweep for collision detection

### Markers & Helpers
- **Marker2D** - position marker (replaces Position2D)
- **RemoteTransform2D** - remotely controls another node's transform
- **VisibleOnScreenNotifier2D** - detects visibility on screen
- **VisibleOnScreenEnabler2D** - enables/disables nodes based on visibility

### Utility Nodes
- **Timer** - fires timeout signal after delay
- **CanvasGroup** - treats children as single item for rendering
- **Node** - base node for non-visual logic

### UI / Control Nodes (also usable in 2D)
- **Control** - base for all UI nodes
- **Label** - text display
- **RichTextLabel** - BBCode-formatted text
- **Button** - clickable button
- **TextureButton** - image-based button
- **LinkButton** - hyperlink-style button
- **CheckBox** - checkbox
- **CheckButton** - toggle switch
- **OptionButton** - dropdown select
- **MenuButton** - dropdown menu trigger
- **SpinBox** - numeric input with arrows
- **HSlider / VSlider** - horizontal/vertical slider
- **HScrollBar / VScrollBar** - scrollbars
- **ProgressBar** - progress indicator
- **TextureProgressBar** - image-based progress bar
- **LineEdit** - single-line text input
- **TextEdit** - multi-line text editor
- **CodeEdit** - code editor with syntax highlighting
- **ColorPickerButton** - color selection
- **ColorRect** - solid color rectangle
- **TextureRect** - displays a texture
- **NinePatchRect** - 9-slice scalable texture
- **Panel** - styled background panel
- **PanelContainer** - panel with child layout
- **TabContainer** - tabbed views
- **TabBar** - tab strip
- **Tree** - hierarchical tree view
- **ItemList** - scrollable item list
- **MenuBar** - menu strip
- **FileDialog** - file picker
- **AcceptDialog** - OK dialog
- **ConfirmationDialog** - OK/Cancel dialog
- **Popup** - popup window
- **PopupMenu** - context menu
- **PopupPanel** - popup with panel
- **Window** - standalone window
- **GraphEdit** - node graph editor
- **GraphNode** - node in graph editor

#### Layout Containers
- **HBoxContainer / VBoxContainer** - horizontal/vertical box layout
- **GridContainer** - grid layout
- **FlowContainer** - wrapping flow layout
- **CenterContainer** - centers child
- **MarginContainer** - adds margins
- **AspectRatioContainer** - maintains aspect ratio
- **SplitContainer / HSplitContainer / VSplitContainer** - resizable split
- **ScrollContainer** - scrollable area
- **SubViewportContainer** - embeds viewport

---

## 6. Bottom Panels

### Output Panel
- **Message categories**: Log (white), Error (red), Warning (yellow), Editor (gray)
- **Category filter buttons** - toggle visibility per category
- **Text filter** - search within output messages
- **Clear button** - manually clear all messages
- **Auto-clear on play** - configurable
- **Auto-open on play** - configurable
- **Copy text** - select and copy output
- **Rich text support** via print_rich() with BBCode

### Debugger Panel Tabs

#### Stack Trace
- Displays call stack when breakpoint hit
- Shows object state/variables
- Step Into / Step Over / Break / Continue buttons
- Skip All Breakpoints toggle
- Copy Error button
- Green triangle shows current execution line

#### Errors Tab
- Lists errors and warnings from running game
- Click to navigate to source line
- Stack trace per error

#### Evaluator Tab
- Expression evaluator (REPL) when paused at breakpoint
- Evaluate constant expressions, member variables, local variables
- "Clear on Run" toggle for persistent results

#### Profiler Tab
- Monitors function execution time
- Self time vs inclusive time
- Sort by various metrics
- Frame-by-frame profiling

#### Visual Profiler Tab
- CPU and GPU framegraph display
- Milliseconds or percentage display
- "Fit to Frame" toggle
- Category-based result filtering

#### Network Profiler Tab
- Lists multiplayer RPC nodes
- Bandwidth usage statistics (incoming/outgoing)

#### Monitors Tab
- Real-time graphs for: FPS, Process Time, Physics Time, Navigation Time
- Memory: Static, Dynamic, Object Count
- Rendering: Objects Drawn, Vertices, Draw Calls
- GPU: VRAM usage
- Physics: Active Objects, Collision Pairs
- Audio: Output Latency
- Navigation: Regions, Agents
- Custom monitors from code
- Values tracked even when tab not visible

#### Video RAM Tab
- Lists resources by path, type, format, VRAM amount
- Sortable columns

#### Misc Tab
- Clicked Control identification at runtime
- Engine meta information

### Animation Panel
- See Section 10 below for full details

### Audio Buses Panel
- Visual audio bus layout editor
- Add/remove/rename buses
- Add effects per bus (reverb, chorus, delay, EQ, limiter, compressor, etc.)
- Volume sliders (dB) per bus
- Mute/Solo/Bypass toggles per bus
- Bus routing (send to other buses)
- Master bus always present
- Save/Load bus layouts

### Shader Editor Panel
- Appears when editing VisualShader or text shaders
- Syntax highlighting for shader language
- Autocomplete for shader functions/uniforms
- Error display

---

## 7. Script Editor

### Core Features
- Fully integrated code editor for GDScript
- **Syntax highlighting** (GDScript, JSON, Plain Text, custom)
- **Auto-completion** of variables, functions, constants, node paths
- **Syntax checking** with real-time error/warning markers
- **Code folding** - fold/unfold blocks, regions, all
- **Bookmarks** - toggle with blue marker, navigate between
- **Breakpoints** - red circle markers, navigate between
- **Multiple carets** - Alt + click to add cursors
- **Inline symbol renaming** - Ctrl+D to select matching symbols
- **Minimap** - clickable overview of entire script
- **Word wrap** toggle
- **Soft/hard guidelines** at 80 and 100 characters
- **Auto-indentation**
- **Customizable theme** / syntax colors
- **Line numbers** in left margin
- **Function override indicator** - shows inherited functions
- **Signal receiver indicator** - shows signal connections
- **Code regions** - named collapsible sections (#region / #endregion)

### File Menu
- New GDScript / New Text File
- Open / Open Recent / Reopen Closed Script
- Save / Save As / Save All
- Soft Reload Tool Script
- Copy Script Path (res://)
- Show in FileSystem
- Navigate History (Previous/Next script)
- Import/Save/Reload Theme
- Close / Close All
- Run File (EditorScript)

### Edit Menu
- Undo / Redo
- Cut / Copy / Paste / Select All
- Duplicate Selection / Duplicate Lines
- **Evaluate Selection** (calculates math expressions)
- Move Line Up / Move Line Down
- Indent / Unindent
- Delete Line
- Toggle Comment
- Fold/Unfold Line / Fold All / Unfold All
- Create Code Region
- Completion Query
- Trim Trailing Whitespace
- Trim Final Newlines
- Convert Indentation (Spaces <-> Tabs)
- Auto Indent Selection
- Convert Case (Upper / Lower / Capitalize)
- Syntax Highlighter Selection

### Search Menu
- **Find** (Ctrl+F) with Match Case, Whole Words options
- Find Next / Find Previous
- **Replace** with single/batch operations
- "Selection Only" checkbox
- **Find in Files** - project-wide search
- **Replace in Files** - project-wide replace with preview
- Results panel showing file count and match count

### Go To Menu
- **Go to Function** - searchable function list
- **Go to Line** (Ctrl+G)
- Toggle Bookmark / Go to Next/Previous Bookmark
- Bookmarks list
- Toggle Breakpoint / Go to Next/Previous Breakpoint

### Debug Menu
- Breakpoint management
- Step Into / Step Over / Continue

### Script Panel (Left Sidebar)
- File type icons
- Relative path tooltips on hover
- Case-insensitive file search
- Sort alphabetically
- Drag to reorder files
- Middle-click to close
- **Method outline** with searchable filter
- Toggle alphabetical vs source order

### Status Bar
- Error/warning count (clickable to navigate)
- Zoom level (Ctrl+scroll to adjust)
- Caret position (line:column)
- Indentation mode indicator

### Script Temperature
- Color-coded file names by edit recency
- Configurable in Editor Settings

---

## 8. FileSystem Dock

### File Browser
- Tree view of project directory structure
- Grid/list view toggle for file display
- **Search bar** - filter files by name
- File type icons
- Folder expand/collapse
- Navigate to res:// root

### File Operations
- **Create** - New Folder, New Script, New Resource, New Scene
- **Rename** files and folders
- **Move** files via drag-drop (updates all references)
- **Delete** files (with confirmation)
- **Duplicate** files
- **Copy Path** / **Copy Absolute Path** / **Copy UID**
- **Show in File Manager** (open OS file browser)

### Resource Integration
- **Drag resources** from dock to Inspector properties
- **Drag scenes** from dock to Scene Tree (instancing)
- **Drag scripts** to nodes (attach script)
- Compatible properties highlight when dragging
- **Preview** on hover for textures/audio/etc.
- **Double-click** to open/edit resource
- **Open in External Program** context menu option

### Import System
- Import dock appears when selecting importable files
- Reimport button
- Import presets (Texture, Audio, etc.)
- Per-file import settings

### Context Menu
- Open / Edit
- Open in External Program
- Rename / Move to / Duplicate / Delete
- New Folder / New Scene / New Script / New Resource
- Copy Path / Copy UID
- Show in File Manager
- Set as Main Scene
- View Owners (what depends on this file)
- Dependencies (what this file depends on)

### Signals (FileSystemDock class)
- files_moved
- folder_moved
- file_removed
- folder_removed
- resource_removed

---

## 9. Signals System

### Node Dock (Right Panel, next to Inspector)
- **Signals tab** - lists all signals available on selected node
- **Groups tab** - lists groups the node belongs to

### Signal List Display
- Organized by class hierarchy (Node signals, then subclass signals)
- Each signal shows name and parameter types
- Connected signals show green connection icon
- Custom signals from scripts appear in list

### Connection Dialog
- **Double-click** signal to open connection window
- **Target node picker** - tree view to select receiver
- **Callback method name** - auto-generated as `_on_[NodeName]_[signal_name]`
- **Advanced mode** toggle:
  - Simple: connect to nodes with scripts, auto-generate callbacks
  - Advanced: connect to any node, use existing functions, add binds
- **Deferred** checkbox - call deferred (next idle frame)
- **One-shot** checkbox - auto-disconnect after first emission
- **Connect** button creates the connection and jumps to script

### Connected Signals Management
- View all connections on a signal
- Disconnect signal connections
- Navigate to connected method in script editor
- Connection indicator icon in script editor gutter

### Custom Signals
- Define in GDScript: `signal my_signal(param1: Type, param2: Type)`
- Emit: `my_signal.emit(value1, value2)`
- Appear in Node dock signal list for connection via UI

### Script Connection
- `node.signal_name.connect(callable)`
- `node.signal_name.disconnect(callable)`
- `node.signal_name.emit(args)`
- Lambdas as receivers: `signal.connect(func(): print("fired"))`

---

## 10. Animation System

### AnimationPlayer Node
- Stores named Animation resources
- Play/stop/pause/seek controls
- Animation library system (multiple libraries per player)
- Autoplay on load (configurable)
- Default blend time between animations
- Root node setting (animations are relative to this)
- Method call track support
- RESET animation for default pose

### Animation Panel (Bottom Panel - appears when AnimationPlayer selected)

#### Top Bar
- **Animation selector** dropdown
- **New Animation** button
- **Load Animation** button
- **Save Animation** button
- **Duplicate Animation** button
- **Rename Animation** button
- **Delete Animation** button
- **Animation library** selector
- **Autoplay** toggle (star icon)
- **Loop** mode: None / Loop / Ping-Pong
- **Animation length** field (seconds)
- **Animation step** (time snap resolution)

#### Playback Controls
- **Play** / **Play Backwards** / **Stop**
- **Seek bar** - drag to scrub timeline
- **Current time** display
- **Speed scale** control

#### Timeline
- Horizontal time ruler
- Zoom in/out on timeline
- Snap to step toggle
- Frame numbers or time display
- Playback position indicator (vertical line)
- Animation length marker

#### Track List (Left Panel)
- Track name (node path + property)
- Track type icon
- **Update mode** per track:
  - Continuous (interpolated every frame)
  - Discrete (value set only at keyframe times)
  - Capture (blend from current value to first keyframe)
- **Interpolation mode** per track:
  - Nearest (no interpolation)
  - Linear
  - Cubic
- Track enable/disable toggle
- Delete track button
- Track context menu

#### Track Types
- **Property Track** - animate any node property
- **Position 3D / Rotation 3D / Scale 3D** - specialized 3D transform tracks
- **Blend Shape Track** - mesh blend shapes
- **Call Method Track** - call functions at specific times
- **Bezier Curve Track** - smooth curves with tangent handles
- **Audio Playback Track** - trigger audio clips
- **Animation Playback Track** - trigger other animations

#### Keyframe Operations
- **Insert keyframe** - click key icon on property in Inspector, or right-click timeline
- **Delete keyframe** - select and press Delete
- **Move keyframe** - drag along timeline
- **Copy/Paste keyframes**
- **Select multiple keyframes** - Shift+click or rectangle select
- **Keyframe context menu** - insert, delete, change easing
- **Easing curves** - visual easing editor per keyframe transition

#### Bezier Curve Editor
- Dedicated view for Bezier tracks
- Tangent handle editing
- Visual curve display
- Auto-smooth tangents option

#### Onion Skinning
- Toggle ghost frames overlay
- Configure past/future frame count
- Opacity control for ghost frames

#### Animation Markers
- Named markers on timeline
- Navigate between markers

### AnimationTree
- **State Machine** mode - states with transitions
- **Blend Tree** mode - blend animations by weight
- **BlendSpace1D / BlendSpace2D** - parameter-driven blending
- Root motion support
- Advance conditions for transitions
- Visual node graph editor for blend trees

### Tween (Code-Based Animation)
- Tween class for programmatic animation
- Property tweens, method tweens, callback tweens
- Easing functions (ease in, ease out, etc.)
- Transition types (linear, sine, quad, cubic, etc.)
- Parallel and sequential composition
- Loops and delays

---

## 11. Additional Editor Features

### Undo/Redo System
- Global undo/redo with history
- Per-scene undo history
- Undo/Redo in Inspector, Scene Tree, Viewport, Script Editor
- Action descriptions in Edit menu

### Project Settings Dialog
- **General** tab - application settings, display, rendering, physics, etc.
- **Input Map** tab - define input actions with key/button/axis bindings
- **Localization** tab - translation management
- **Autoload** tab - singleton/autoload scripts
- **Plugins** tab - enable/disable editor plugins
- **Shader Globals** tab - global shader parameters

### Editor Settings
- Hundreds of configurable options
- Interface, FileSystem, Editors, Network categories
- Theme customization
- Keyboard shortcut customization
- External editor configuration

### Version Control (VCS)
- Built-in Git integration
- Diff viewer
- Commit interface
- Branch management

### Export System
- Export presets per platform
- Resource filtering
- Feature tags
- One-click export

---

## Appendix A: All Godot Variant Types (Inspector Must Handle)

1. NIL
2. BOOL
3. INT
4. FLOAT
5. STRING
6. VECTOR2
7. VECTOR2I
8. RECT2
9. RECT2I
10. VECTOR3
11. VECTOR3I
12. TRANSFORM2D
13. VECTOR4
14. VECTOR4I
15. PLANE
16. QUATERNION
17. AABB
18. BASIS
19. TRANSFORM3D
20. PROJECTION
21. COLOR
22. STRING_NAME
23. NODE_PATH
24. RID
25. OBJECT
26. CALLABLE
27. SIGNAL
28. DICTIONARY
29. ARRAY
30. PACKED_BYTE_ARRAY
31. PACKED_INT32_ARRAY
32. PACKED_INT64_ARRAY
33. PACKED_FLOAT32_ARRAY
34. PACKED_FLOAT64_ARRAY
35. PACKED_STRING_ARRAY
36. PACKED_VECTOR2_ARRAY
37. PACKED_VECTOR3_ARRAY
38. PACKED_COLOR_ARRAY
39. PACKED_VECTOR4_ARRAY

---

## Appendix B: Keyboard Shortcuts (Key Mappings)

### General
- Ctrl+Z - Undo
- Ctrl+Shift+Z / Ctrl+Y - Redo
- Ctrl+S - Save Scene
- Ctrl+Shift+S - Save All
- Ctrl+A - Select All (in context)
- Delete - Delete selected
- F1 - Search Help
- F5 - Run Project
- F6 - Run Current Scene
- F7 - Pause
- F8 - Stop

### Scene Tree
- Ctrl+D - Duplicate
- Ctrl+C / Ctrl+V - Copy / Paste
- F2 - Rename
- Delete - Delete node

### 2D Viewport
- Q - Select Mode
- W - Move Mode
- E - Rotate Mode
- S - Scale Mode
- R - Ruler Mode
- H - Pan Mode
- Ctrl+Shift+F - Frame Selection
- 1-9 - Zoom levels

### Script Editor
- Ctrl+F - Find
- Ctrl+H - Replace
- Ctrl+Shift+F - Find in Files
- Ctrl+G - Go to Line
- Ctrl+K - Toggle Comment
- Ctrl+D - Select Next Occurrence
- Tab / Shift+Tab - Indent / Unindent
- Ctrl+/ - Toggle Comment
- Ctrl+Shift+K - Delete Line
- Alt+Up/Down - Move Line Up/Down
