# rView — English UI strings: the source language and the fallback for every translation.
# Syntax: https://projectfluent.org/fluent/guide/  ·  How to add a language: README.md in this folder.
# Menu texts: "&" marks the mnemonic letter, "&&" is a literal "&".

## Menus

menu-file = &File
menu-edit = &Edit
menu-view = &View
menu-go = &Go
menu-markup = &Markup
menu-tools = &Tools
menu-animation = &GIFs and animated images
menu-help = &Help
menu-recent = Open &Recent
menu-openwith = Open &With

## Actions (menu texts; the id is "action-" + the action's stable key)

action-open = &Open...
action-openurl = Open &URL Link Image...
action-reloadfile = Re&load File
action-opencontainingfolder = Show in E&xplorer
action-showfileinfo = Show File &Info
action-undo = &Undo Delete
action-copy = &Copy
action-copybase64 = Copy as &Base64
action-paste = &Paste
action-rename = R&ename...
action-delete = &Delete
action-deletepermanent = Delete Permanen&tly
action-firstfile = &First File
action-previousfile = Previous Fi&le
action-nextfile = &Next File
action-lastfile = Las&t File
action-randomfile = &Random File
action-zoomin = Zoom &In
action-zoomout = Zoom &Out
action-originalsize = Ori&ginal Size
action-rotateright = Rotate &Right
action-rotateleft = Rotate &Left
action-mirror = &Mirror
action-flip = &Flip
action-fullscreen = Enter F&ull Screen
action-fullscreen-exit = Exit F&ull Screen
action-undo-restore = &Restore Original
action-saveframeas = Save Frame &As...
action-pause = Pa&use
action-pause-resume = Res&ume
action-previousframe = &Previous Frame
action-nextframe = &Next Frame
action-decreasespeed = &Decrease Speed
action-resetspeed = &Reset Speed
action-increasespeed = &Increase Speed
action-slideshow = Start S&lideshow
action-slideshow-stop = Stop S&lideshow
action-colorpicker = &Color Picker
action-convert = Con&vert...
action-options = &Settings
action-quit = E&xit
action-closewindow = &Close Window
action-welcome = &Welcome
action-clearrecents = &Clear Menu
action-openwithother = &Choose another app
action-markup = &Markup
action-crop = Cro&p
action-saveproject = &Save Project
action-export = &Export...
action-overwriteoriginal = &Overwrite Original...
action-redo = &Redo

## Viewer

error-opening =
    Error occurred opening
    { $name }
    { $message } (Error { $code })
filter-images = Supported Images
filter-all = All Files
filter-project = rView Project

## Buttons

button-ok = OK
button-cancel = Cancel
button-apply = Apply
button-close = Close
button-restore-defaults = Restore Defaults

## Message box titles

title-openurl = Open URL Link Image
title-rename = Rename
title-delete = Delete
title-delete-permanent = Delete Permanently
title-undo = Undo Delete
title-recent = Open Recent
title-copy = Copy
title-watermark = Watermark
title-base64 = Base64
title-explorer = Show in Explorer
title-openwith = Open With
title-saveframe = Save Frame As
title-markup = Markup
title-save-project = Save Project
title-export = Export
title-convert = Convert
title-overwrite = Overwrite Original
title-settings = Settings

## Rename

rename-title = Rename...
rename-label = File name:
rename-empty = The file name cannot be empty.
rename-bad-char = A file name can't contain the character { $char }.
rename-bad-end = A file name can't end with a dot or a space.
rename-read-only = { $name } is read-only.
rename-exists = A file named { $name } already exists.
rename-failed =
    Can't rename { $old } to { $new }:
    { $reason }

## Open URL

openurl-title = Open URL Link Image
openurl-label = URL Link Image:
openurl-invalid = { $url } is not an http or https URL, or a Base64 data URL.
base64-failed =
    Can't open the Base64 image:
    { $reason }
base64-not-image = The data is not an image in a supported format.
base64-project = A project can't be copied as Base64: it contains the whole original image, including what is under its redactions. Export an image and copy that.
download-title = Downloading...
download-too-large = The file is larger than { $mib } MiB.
download-failed =
    Can't download { $url }:
    { $reason }

## Delete and Undo Delete

delete-read-only = Can't delete { $name }: the file is read-only.
delete-no-bin = { $name } can't be moved to the Recycle Bin: this drive has none. Use Delete Permanently.
delete-confirm = Move { $name } to the Recycle Bin?
delete-confirm-detail = Undo Delete (Ctrl+Z) puts it back.
delete-permanent-confirm = Delete { $name } permanently?
delete-permanent-detail = It won't be moved to the Recycle Bin: it will be erased for good and can't be recovered. This cannot be undone.
delete-dont-ask = Do not ask again
delete-failed =
    Can't delete { $name }:
    { $reason }
delete-not-in-bin = { $name } was deleted but is not in the Recycle Bin: it can't be restored.
undo-failed =
    Can't restore { $name }:
    { $reason }
bin-not-moved = { $path } was not moved to the Recycle Bin.
bin-path-too-long = { $path } has a path of 260 characters or more: Windows can't put it in the Recycle Bin (it would delete it for good), so it was left where it is. Use Delete Permanently, or move it to a shorter folder first.
bin-gone = The file is no longer in the Recycle Bin.

## Files, clipboard, shell

recent-missing = { $path } no longer exists.
file-exists = { $path } already exists.
file-not-found = { $path } not found.
clipboard-busy = The clipboard is in use by another program.
clipboard-refused = The clipboard refused the data (format { $format }).
openwith-gone = { $app } is no longer available.

## File Info

info-title = File Info
info-name = Name
info-type = Type
info-type-value = { $format } image
info-location = Location
info-size = Size
info-bytes = { $n } bytes
info-modified = Modified
info-dimensions = Dimensions
info-aspect = Aspect ratio
info-frames = Frames
info-profile = Colour profile
info-profile-assumed = sRGB (assumed)
info-profile-unnamed = Embedded ICC profile
info-depth = Bit depth
info-depth-value = { $bits } bits per channel
info-depth-hdr = { $bits } bits per channel (HDR shown as SDR)
info-exif = EXIF
info-exif-gps = Yes, with GPS position
info-exif-yes = Yes, no GPS position
info-exif-no = No
info-exif-unread = Not read for this format
info-tab-general = General
info-tab-image = Image
info-tab-camera = Camera
info-tab-metadata = Metadata
info-created = Created
info-megapixels = Megapixels
info-orientation = Orientation
info-orientation-landscape = Landscape
info-orientation-portrait = Portrait
info-orientation-square = Square
info-depth-float = { $bits }-bit floating point per channel
info-channels = Channels
info-channels-gray = Grayscale
info-channels-gray-alpha = Grayscale + alpha
info-channels-rgb = RGB
info-channels-rgba = RGBA
info-channels-indexed = Indexed (palette)
info-channels-indexed-alpha = Indexed (palette) with transparency
info-channels-cmyk = CMYK
info-has-alpha = Has alpha
info-transparency = Alpha transparency
info-transparency-none = None (fully opaque)
info-transparency-binary = On/off only (no semi-transparent pixels)
info-transparency-partial = Semi-transparent pixels
info-alpha-mode = Alpha mode
info-alpha-straight = Straight
info-alpha-premultiplied = Premultiplied
info-unique-colors = Unique colours
info-color-space = Colour space
info-color-space-uncalibrated = Uncalibrated
info-icc-size = ICC profile size
info-resolution = DPI / PPI
info-resolution-value = { $x } x { $y } DPI
info-resolution-aspect = { $x } : { $y } (pixel aspect only)
info-resolution-unit = DPI unit
info-unit-inch = Inch
info-unit-cm = Centimetre (shown converted to DPI)
info-unit-none = None
info-jpeg-sampling = JPEG sampling
info-png-chunks = PNG chunks
info-camera-make = Camera make
info-camera-model = Camera model
info-lens = Lens model
info-date-taken = Date taken
info-exposure = Exposure time
info-f-number = F-number
info-iso = ISO
info-focal-length = Focal length
info-flash = Flash
info-gps = GPS coordinates
info-software = Software
info-copyright = Copyright
info-artist = Artist / Author
info-iptc = IPTC metadata
info-xmp = XMP metadata
info-thumbnail = Embedded thumbnail
info-thumbnail-size = Yes, { $w } x { $h }
info-yes = Yes
info-no = No
info-copy = Copy
info-copied = Copied
info-reading = Reading the file…
info-no-camera = No camera information in this file.
info-nothing = Nothing to show.

## Language (every language file names itself here; shown in Settings → Language)

language-name = English

## Settings window

settings-tab-window = Window
settings-tab-image = Image
settings-tab-misc = Miscellaneous
settings-tab-shortcuts = Shortcuts
settings-bgcolor = Background colour:
settings-bgcolor-custom = Custom colour (off: the system window colour)
settings-titlebarmode = Title bar text:
settings-titlebar-full = File name and size
settings-titlebar-name = File name
settings-windowresizemode = Resize the window to the image:
settings-resize-never = Never
settings-resize-launching = When launching
settings-resize-opening = When opening images
settings-resize-limits = Window size (of the screen):
settings-menubar = Show the menu bar
settings-fullscreendetails = Show the title in full screen
settings-filtering = Smooth images when zoomed in (filtering)
settings-scaling = Smooth images when zoomed out (scaling)
settings-scalingtwo = Smooth scaling above 100 %
settings-scalefactor = Zoom step:
settings-scrollzoom = Mouse wheel:
settings-scroll-zoom = Zooms
settings-scroll-auto = Zooms (mouse), scrolls (touchpad)
settings-scroll-scroll = Scrolls
settings-fractionalzoom = Fractional zoom with high-resolution wheels
settings-cursorzoom = Zoom around the mouse pointer
settings-cropmode = Fit the image to:
settings-crop-whole = The whole window
settings-crop-height = The window height
settings-crop-width = The window width
settings-pastactualsize = Fit small images past their actual size
settings-colorspace = Colour space conversion:
settings-colorspace-disabled = Disabled
settings-colorspace-auto = Automatic
settings-colorspace-srgb = sRGB
settings-colorspace-p3 = Display P3
settings-language = Language:
settings-language-system = System default
settings-sortmode = Sort files by:
settings-sort-name = Name
settings-sort-modified = Date modified
settings-sort-created = Date created
settings-sort-size = Size
settings-sort-type = Type
settings-sort-random = Random
settings-sortdescending = Descending
settings-preloading = Preloading:
settings-preload-off = Off
settings-preload-adjacent = Adjacent files
settings-preload-extended = Extended
settings-loopfolders = Loop through folders
settings-slideshow = Slideshow interval:
settings-slideshowreversed = Reversed
settings-afterdelete = After deleting a file:
settings-afterdelete-back = Show the previous file
settings-afterdelete-nothing = Show nothing
settings-afterdelete-forward = Show the next file
settings-askdelete = Ask before moving files to the Recycle Bin
settings-mime = Detect images by their content, not only the extension
settings-saverecents = Remember recent files
settings-updates = Check for updates at startup
settings-skiphidden = Skip hidden files
settings-markup = Markup
settings-metadata = Metadata on export:
settings-metadata-keep = Keep
settings-metadata-location = Remove the location
settings-metadata-all = Remove all
settings-jpegquality = JPEG quality:
settings-askoverwrite = Ask before overwriting the original
settings-shortcut-hint = Double-click an action to record a new shortcut.
settings-shortcut-recording = Press the new shortcut for { $action }...
settings-shortcut-clear = No shortcut
settings-shortcut-action = Action
settings-shortcut-keys = Shortcuts
settings-shortcut-conflict = Also used by: { $actions }

## About, Welcome, updates

empty-version = Version { $version }
empty-open = Right-click anywhere and choose Open... (Ctrl+O)
empty-drop = or drag & drop an image onto this window
welcome-title = Welcome
welcome-heading = Welcome to rView
welcome-text = rView opens images quickly and gets out of the way: open a file, then use the arrow keys to move through its folder.
welcome-tips = Right-click the image for every action; Settings is in the Tools menu.
welcome-default = Open images with rView by default?
welcome-default-button = Set as default…
welcome-default-detail = Windows asks you to confirm in Settings > Default apps.
welcome-updates = Check for updates at startup
welcome-updates-detail = rView asks GitHub for its latest release at most every 4 hours; nothing else is sent.
welcome-start = Get started
color-title = Color Picker
color-pick = Pick from image
color-picking = Click the image to pick a color. Esc stops.
color-recent = Recent colors
color-recent-clear = Clear
color-recent-empty = Colors you pick or copy appear here.
color-copy = Copy
color-copied = Copied
markup-move-bar = Drag to move the toolbar
update-title = Update available
update-available = rView { $version } is available
update-current = You have version { $version }.
update-changelog = What's new:
update-download = Download
update-status-off = Update notifications are off.
update-status-recent = Last checked for updates on { $when }.
update-status-checking = Checking for updates...
update-status-uptodate = rView is up to date.
update-status-available = Version { $version } is available.
update-status-failed = The update check failed: { $reason }

## Markup

markup-tool-select = Select
markup-tool-crop = Crop
markup-tool-rect = Rectangle
markup-tool-ellipse = Ellipse
markup-tool-line = Line
markup-tool-arrow = Arrow
markup-tool-pen = Pen
markup-kind-rect = Rectangle
markup-kind-ellipse = Ellipse
markup-kind-line = Line
markup-kind-arrow = Arrow
markup-kind-pen = Pen stroke
markup-kind-highlight = Highlight
markup-kind-text = Text
markup-arrow-curve = Curve
markup-confirm = Confirm
markup-confirm-hint = Drag it to move, drag the handles to resize or rotate, change it here. Enter (or Esc) confirms; Delete removes it. Until then clicks don't add new elements.
markup-kind-loupe = Loupe
markup-tool-loupe = Loupe
markup-loupe-zoom = Zoom
markup-fonts-missing = Not installed on this PC, shown with Lato: { $fonts }
markup-kind-redaction = Redaction
markup-tool-text = Text
markup-tool-adjust = Adjust
markup-tool-redact = Redact
markup-text-colour = Colour
markup-text-background = Background
markup-font = Font
markup-font-size = Size
markup-align-left = Left
markup-align-center = Centre
markup-align-right = Right
markup-redact-mode = Mode
markup-redact-solid = Solid
markup-redact-blur = Blur
markup-redact-pixelate = Pixelate
markup-redact-radius = Radius
markup-redact-block = Block
markup-redact-colour = Colour
markup-redact-shape = Shape
markup-protect-solid = Opaque cover, irreversible in the exported image.
markup-protect-blur = A visual effect. Don't use it to hide sensitive data: use Solid.
markup-protect-pixelate = Reduces legibility but doesn't guarantee protection. For sensitive data use Solid.
markup-fill-not-redaction = A filled shape is an annotation: its anti-aliased edges mix with the pixels below. To hide data use the Redact tool (Solid).
markup-rotate-left = Rotate left
markup-rotate-right = Rotate right
markup-flip-h = Flip H
markup-flip-v = Flip V
markup-undo = Undo
markup-redo = Redo
markup-done = Done
markup-inspector = Inspector
markup-hint = Pick a tool, or click an object to select it. Shift+click adds to the selection, Alt+click reaches the object below.
markup-n-selected = { $n } objects selected
markup-mixed = Different styles: a change applies to all of them.
markup-duplicate = Duplicate
markup-delete = Delete
markup-stroke = Outline
markup-fill = Fill
markup-width = Width
markup-opacity = Opacity
markup-corners = Rounded corners
markup-arrow-both = Heads at both ends
markup-crop-free = Free
markup-crop-original = Original
markup-crop-1-1 = 1:1
markup-crop-4-3 = 4:3
markup-crop-3-2 = 3:2
markup-crop-16-9 = 16:9
markup-crop-social = Social media
markup-crop-x-post = X post (16:9)
markup-crop-x-header = X header (3:1)
markup-crop-facebook-post = Facebook post (1.91:1)
markup-crop-facebook-story = Facebook story (9:16)
markup-crop-instagram-square = Instagram square (1:1)
markup-crop-instagram-portrait = Instagram portrait (4:5)
markup-crop-instagram-landscape = Instagram landscape (1.91:1)
markup-crop-instagram-story = Instagram story / reel (9:16)
markup-crop-youtube-thumbnail = YouTube thumbnail (16:9)
markup-crop-linkedin-post = LinkedIn post (1.91:1)
markup-crop-linkedin-banner = LinkedIn banner (4:1)
markup-crop-custom = Custom
markup-crop-swap = Swap
markup-crop-size = { $w } × { $h } px
markup-crop-apply = Apply
markup-crop-reset = Reset
markup-adjust-exposure = Exposure
markup-watermark = Watermark
markup-compare = Compare before/after
markup-compare-before = Before
markup-compare-nothing = Nothing to compare yet: adjust the image, add a redaction or a shape first.
markup-compare-after = After
markup-watermark-none = No recent watermarks
markup-watermark-choose = Choose image...
markup-watermark-clear = Clear list
markup-watermark-missing = { $path } is no longer there.
markup-watermark-failed =
    { $name } can't be used as a watermark and was removed from the list:
    { $reason }
markup-kind-image = Image
markup-adjust-brightness = Brightness
markup-adjust-contrast = Contrast
markup-adjust-saturation = Saturation
markup-adjust-temperature = Temperature
markup-adjust-hue = Hue
markup-adjust-sharpness = Sharpness
markup-adjust-reset = Reset all
markup-adjust-note = Only the image is adjusted: shapes and text keep their colours, a Solid redaction its exact colour. Double-click a slider to reset it.
markup-unsaved = There are unsaved changes.
markup-unsaved-detail = Leaving Markup discards them.
markup-unsaved-exported = The image was exported, but the changes are not saved as a project: shapes and redactions won't be editable any more.
markup-discard = Discard changes
markup-export = Export…
markup-save-project = Save Project
markup-export-first = Export…
save-project-title = Save Project (it contains the whole original image: keep it private)
export-title = Export
export-format = Format
export-quality = JPEG quality
export-background = Background
export-metadata = Metadata
export-jpeg-note = JPEG is recompressed with some loss of quality; transparent areas get the background colour.
export-no-exif-note = This format keeps no camera metadata (EXIF).
export-redactions-note = The image has redactions: all metadata is removed by default. The original file and the project (.rvm) still contain what is under them. Only Solid hides data: Blur and Pixelate are visual effects; JPEG compression may shift the colour of a Solid area's edges by a few units.
export-over-project = A project cannot be replaced by an exported image. Choose another name.
export-transparency = Transparency
export-flatten = Replace with the background colour
export-lossless-note = { $format } is lossless: the quality setting applies to JPEG only.
export-resolution = Resolution
export-dpi-unchanged = Unchanged
export-dpi-custom = Custom
export-dpi-value = { $dpi } DPI
export-dpi-note = Only the resolution stored in the file changes: the pixels are not resampled.
export-dpi-webp-note = WebP stores the resolution only in EXIF metadata: “Strip all” removes it.
export-size = Size
export-size-original = Original
export-size-pixels = Pixels
export-size-percent = Percentage
export-size-print = Print size
export-keep-ratio = Keep proportions
export-size-result = ({ $w } × { $h } px)
export-size-too-large = This size is too large: an enlarged image can have at most 268 million pixels.
export-print-note = The image is resampled to the print size at this resolution, which is stored in the file.
convert-title = Convert
convert-over-source = The image cannot be converted onto itself. Choose another name.
overwrite-confirm = Replace “{ $name }” with the edited image?
overwrite-detail = The previous file goes to the Recycle Bin: Restore Original (Edit menu) brings it back.
overwrite-jpeg-detail = JPEG is recompressed with some loss of quality. The previous file goes to the Recycle Bin: Restore Original (Edit menu) brings it back.
