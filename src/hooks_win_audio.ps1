# Windows audio helper for revoiced. Windows has no built-in mp3 player on the
# command line and no stdlib TTS, so both go through .NET here.
#
#   win_audio.ps1 -Mode play -Path song.mp3
#   win_audio.ps1 -Mode say  -Text "hello"

param(
    [ValidateSet('play', 'say')][string]$Mode,
    [string]$Path,
    [string]$Text
)

$ErrorActionPreference = 'Stop'

if ($Mode -eq 'play') {
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
