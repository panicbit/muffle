# muffle - mute unwanted pipewire audio connections

The problem: Discord screensharing on Linux records the audio of _all_
applications and does not provide a way to select or filter them.

Muffle allows exactly that: filtering which apps Discord is able to record.

# How it works (technical)

When screensharing, Discord actually already records individual pipewire outputs
from applications individually (as opposed to just monitoring an entire device).
This circumstance allows disconnecting individual connections from application
outputs to Discord.
