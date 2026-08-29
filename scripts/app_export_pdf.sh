#!/usr/bin/env bash
# Export an iWork document to PDF using the real app via AppleScript (no GUI
# interaction needed; dismisses first-launch modals via Accessibility).
#   usage: app_export_pdf.sh <doc.pages|numbers|key> </tmp/out.pdf>
set -euo pipefail
doc=$(realpath "$1"); out=$(realpath -m "$2")
case "${doc##*.}" in
  key) app="com.apple.Keynote" ;;
  numbers) app="com.apple.Numbers" ;;
  pages) app="com.apple.Pages" ;;
  *) echo "unknown extension: $doc" >&2; exit 2 ;;
esac

osascript - "$app" "$doc" "$out" <<'EOF'
on run argv
  set appID to item 1 of argv as text
  set docPath to POSIX file (item 2 of argv)
  set outPath to POSIX file (item 3 of argv)
  -- dismiss any first-launch modal (e.g. "What's New") that blocks Apple events
  tell application "System Events"
    try
      tell (first application process whose bundle identifier is appID)
        if (count of windows) > 0 then
          try
            click button "OK" of window 1
          end try
        end if
      end tell
    end try
  end tell
  tell application id appID
    activate
    open docPath
    set deadline to (current date) + 20
    repeat until ((count of documents) > 0) or ((current date) > deadline)
      delay 0.5
    end repeat
    if (count of documents) is 0 then error "document did not open"
    export document 1 to outPath as PDF
    close document 1 saving no
  end tell
end run
EOF
ls -la "$out"
