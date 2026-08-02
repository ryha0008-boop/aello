# Windows helper for revoiced. Windows has no built-in mp3 player on the
# command line, no stdlib TTS, and no way to raise a desktop notification from
# Python without a dependency - so all three go through .NET and WinRT here.
#
#   win_audio.ps1 -Mode play  -Path song.mp3
#   win_audio.ps1 -Mode say   -Text "hello"
#   win_audio.ps1 -Mode toast -Title "revoiced" -Text "…" -Launch "http://…"

param(
    [ValidateSet('play', 'say', 'toast')][string]$Mode,
    [string]$Path,
    [string]$Text,
    [string]$Xml,
    [string]$Aumid
)

$ErrorActionPreference = 'Stop'

if ($Mode -eq 'toast') {
    # The toast XML is built in Python and handed over whole - escaping,
    # buttons and all - so this stays the thin shim the conventions ask for
    # and nothing about the notification is decided in PowerShell.
    #
    # A toast is always shown *as* some application, and one shown under an
    # AppUserModelID that does not exist is dropped with no error at all, so
    # the caller passes an id it has already registered.
    [void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
    [void][Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]
    $doc = New-Object Windows.Data.Xml.Dom.XmlDocument
    $doc.LoadXml($Xml)
    $toast = New-Object Windows.UI.Notifications.ToastNotification $doc
    [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($Aumid).Show($toast)
} elseif ($Mode -eq 'play') {
    Add-Type -AssemblyName presentationCore
    $player = New-Object System.Windows.Media.MediaPlayer
    $player.Open([uri]$Path)
    $waited = 0
    while (-not $player.NaturalDuration.HasTimeSpan -and $waited -lt 50) {
        Start-Sleep -Milliseconds 100
        $waited++
    }
    $player.Play()
    if ($player.NaturalDuration.HasTimeSpan) {
        Start-Sleep -Milliseconds ([int]($player.NaturalDuration.TimeSpan.TotalMilliseconds + 500))
    }
    $player.Stop()
    $player.Close()
} else {
    Add-Type -AssemblyName System.Speech
    $synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
    $synth.Rate = 1
    $synth.Speak($Text)
    $synth.Dispose()
}
