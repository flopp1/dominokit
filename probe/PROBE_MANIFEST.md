# probe.mid manifest

Format 1, PPQ 480, 9 tracks. Ticks are absolute from song start. Note-offs are omitted (implied by gate). Raw bytes are the SMF event bytes.

## Track 0: Conductor  · channel —

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 09 43 6f 6e 64 75 63 74 6f 72` | track name 'Conductor' |
| 0 | `ff 00 02 00 07` | SEQUENCE NUMBER 7 |
| 0 | `ff 02 0f 50 72 6f 62 65 20 63 6f 70 79 72 69 67 68 74` | COPYRIGHT 'Probe copyright' |
| 0 | `ff 54 05 21 02 03 04 05` | SMPTE OFFSET hr=0x21(25fps,1h) mn=2 se=3 fr=4 ff=5 |
| 0 | `ff 51 03 07 a1 20` | TEMPO 120 BPM (500000 us/qn) |
| 0 | `ff 58 04 04 02 18 08` | TIME SIG 4/4 (dd=2, cc=24, bb=8) |
| 0 | `ff 59 02 00 00` | KEY SIG sf=0 mi=0 (C major) |
| 480 | `ff 51 03 07 6a 73` | TEMPO 123.456 BPM (486003 us/qn) |
| 1920 | `ff 51 03 06 dd d1` | TEMPO 133.333 BPM (450001 us/qn) |
| 1920 | `ff 59 02 01 00` | KEY SIG sf=1 mi=0 (G major) |
| 3840 | `ff 51 03 0a 1d c7` | TEMPO 90.5 BPM (662983 us/qn) |
| 3840 | `ff 59 02 fa 01` | KEY SIG sf=-6 mi=1 (Eb minor) |
| 5760 | `ff 51 03 03 0d 40` | TEMPO 300 BPM (200000 us/qn) |
| 5760 | `ff 59 02 06 00` | KEY SIG sf=6 mi=0 (F# major) |
| 7680 | `ff 51 03 0f 42 40` | TEMPO 60 BPM (1000000 us/qn) |
| 7680 | `ff 59 02 f9 00` | KEY SIG sf=-7 mi=0 (Cb major) |
| 9600 | `ff 51 03 07 a1 20` | TEMPO 120 BPM (500000 us/qn) |
| 9600 | `ff 59 02 00 01` | KEY SIG sf=0 mi=1 (A minor) |
| 10080 | `ff 59 02 07 01` | KEY SIG sf=7 mi=1 (A# minor) |
| 10560 | `ff 51 03 7a 12 00` | TEMPO 7.5 BPM (8000000 us/qn, slow extreme) |
| 11040 | `ff 59 02 ff 00` | KEY SIG sf=-1 mi=0 (F major) |
| 11520 | `ff 01 0a 54 45 58 54 2d 61 73 63 69 69` | TEXT 'TEXT-ascii' |
| 11640 | `ff 05 0b 4c 59 52 49 43 2d 61 73 63 69 69` | LYRIC 'LYRIC-ascii' |
| 11760 | `ff 06 0c 4d 41 52 4b 45 52 2d 61 73 63 69 69` | MARKER 'MARKER-ascii' |
| 11880 | `ff 07 09 43 55 45 2d 61 73 63 69 69` | CUE POINT 'CUE-ascii' |
| 12000 | `ff 04 0a 49 4e 53 54 52 2d 6e 61 6d 65` | INSTRUMENT NAME 'INSTR-name' |
| 12120 | `ff 08 08 50 52 4f 47 4e 41 4d 45` | PROGRAM NAME 'PROGNAME' |
| 12240 | `ff 09 07 44 45 56 4e 41 4d 45` | DEVICE NAME 'DEVNAME' |
| 12360 | `ff 7f 05 00 00 41 12 34` | SEQUENCER-SPECIFIC 00 00 41 12 34 |
| 12480 | `ff 01 06 83 65 83 58 83 67` | TEXT Shift-JIS 'テスト' (83 65 83 58 83 67) |
| 12600 | `ff 01 04 63 61 66 e9` | TEXT Latin-1 'café' (63 61 66 E9) |
| 12720 | `ff 01 06 e6 97 a5 e6 9c ac` | TEXT UTF-8 '日本' (E6 97 A5 E6 9C AC) |
| 12840 | `ff 06 08 83 7d 81 5b 83 4a 81 5b` | MARKER Shift-JIS 'マーカー' |
| 12960 | `ff 01 00` | TEXT empty |
| 15360 | `ff 58 04 03 02 18 08` | TIME SIG 3/4 (dd=2, cc=24) |
| 16800 | `ff 58 04 06 03 0c 08` | TIME SIG 6/8 (dd=3, cc=12) |
| 18240 | `ff 58 04 07 04 06 08` | TIME SIG 7/16 (dd=4, cc=6) |
| 19080 | `ff 58 04 05 01 30 08` | TIME SIG 5/2 (dd=1, cc=48) |
| 23880 | `ff 58 04 0d 03 0c 08` | TIME SIG 13/8 (dd=3, cc=12) |
| 27000 | `ff 58 04 04 02 18 08` | TIME SIG 4/4 (dd=2, cc=24) |

## Track 1: Notes ch1  · channel 1 (0-based 0)

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 09 4e 6f 74 65 73 20 63 68 31` | track name 'Notes ch1' |
| 0 | `90 3c 64` | NOTE key=60 vel=100 gate=480 (off vel 64) |
| 480 | `90 00 01` | NOTE key=0 vel=1 gate=1 (off vel 64) |
| 960 | `90 7f 7f` | NOTE key=127 vel=127 gate=1920 (off vel 64) |
| 1920 | `90 3c 50` | NOTE key=60 vel=80 gate=240 (off vel 64) |
| 1920 | `90 40 51` | NOTE key=64 vel=81 gate=480 (off vel 64) |
| 1920 | `90 43 52` | NOTE key=67 vel=82 gate=960 (off vel 64) |
| 3840 | `90 3e 5a` | NOTE key=62 vel=90 gate=100 (off = note-on vel 0) |
| 4320 | `90 40 46` | NOTE key=64 vel=70 gate=333 (off vel 127) |
| 5760 | `90 41 32` | NOTE key=65 vel=50 gate=960 (off vel 64) |
| 6000 | `90 42 33` | NOTE key=66 vel=51 gate=240 (off vel 64) |
| 7680 | `90 48 63` | NOTE key=72 vel=99 gate=65537 (off vel 64) |

## Track 2: CC plain ch1  · channel 1 (0-based 0)

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 0c 43 43 20 70 6c 61 69 6e 20 63 68 31` | track name 'CC plain ch1' |
| 120 | `b0 01 30` | CC#1 = 48  (tick = cc*120) |
| 240 | `b0 02 55` | CC#2 = 85  (tick = cc*120) |
| 360 | `b0 03 7a` | CC#3 = 122  (tick = cc*120) |
| 480 | `b0 04 1f` | CC#4 = 31  (tick = cc*120) |
| 600 | `b0 05 44` | CC#5 = 68  (tick = cc*120) |
| 840 | `b0 07 0e` | CC#7 = 14  (tick = cc*120) |
| 960 | `b0 08 33` | CC#8 = 51  (tick = cc*120) |
| 1080 | `b0 09 58` | CC#9 = 88  (tick = cc*120) |
| 1200 | `b0 0a 7d` | CC#10 = 125  (tick = cc*120) |
| 1320 | `b0 0b 22` | CC#11 = 34  (tick = cc*120) |
| 1440 | `b0 0c 47` | CC#12 = 71  (tick = cc*120) |
| 1560 | `b0 0d 6c` | CC#13 = 108  (tick = cc*120) |
| 1680 | `b0 0e 11` | CC#14 = 17  (tick = cc*120) |
| 1800 | `b0 0f 36` | CC#15 = 54  (tick = cc*120) |
| 1920 | `b0 10 5b` | CC#16 = 91  (tick = cc*120) |
| 2040 | `b0 11 00` | CC#17 = 0  (tick = cc*120) |
| 2160 | `b0 12 25` | CC#18 = 37  (tick = cc*120) |
| 2280 | `b0 13 4a` | CC#19 = 74  (tick = cc*120) |
| 2400 | `b0 14 6f` | CC#20 = 111  (tick = cc*120) |
| 2520 | `b0 15 14` | CC#21 = 20  (tick = cc*120) |
| 2640 | `b0 16 39` | CC#22 = 57  (tick = cc*120) |
| 2760 | `b0 17 5e` | CC#23 = 94  (tick = cc*120) |
| 2880 | `b0 18 03` | CC#24 = 3  (tick = cc*120) |
| 3000 | `b0 19 28` | CC#25 = 40  (tick = cc*120) |
| 3120 | `b0 1a 4d` | CC#26 = 77  (tick = cc*120) |
| 3240 | `b0 1b 72` | CC#27 = 114  (tick = cc*120) |
| 3360 | `b0 1c 17` | CC#28 = 23  (tick = cc*120) |
| 3480 | `b0 1d 3c` | CC#29 = 60  (tick = cc*120) |
| 3600 | `b0 1e 61` | CC#30 = 97  (tick = cc*120) |
| 3720 | `b0 1f 06` | CC#31 = 6  (tick = cc*120) |
| 3960 | `b0 21 50` | CC#33 = 80  (tick = cc*120) |
| 4080 | `b0 22 75` | CC#34 = 117  (tick = cc*120) |
| 4200 | `b0 23 1a` | CC#35 = 26  (tick = cc*120) |
| 4320 | `b0 24 3f` | CC#36 = 63  (tick = cc*120) |
| 4440 | `b0 25 64` | CC#37 = 100  (tick = cc*120) |
| 4680 | `b0 27 2e` | CC#39 = 46  (tick = cc*120) |
| 4800 | `b0 28 53` | CC#40 = 83  (tick = cc*120) |
| 4920 | `b0 29 78` | CC#41 = 120  (tick = cc*120) |
| 5040 | `b0 2a 1d` | CC#42 = 29  (tick = cc*120) |
| 5160 | `b0 2b 42` | CC#43 = 66  (tick = cc*120) |
| 5280 | `b0 2c 67` | CC#44 = 103  (tick = cc*120) |
| 5400 | `b0 2d 0c` | CC#45 = 12  (tick = cc*120) |
| 5520 | `b0 2e 31` | CC#46 = 49  (tick = cc*120) |
| 5640 | `b0 2f 56` | CC#47 = 86  (tick = cc*120) |
| 5760 | `b0 30 7b` | CC#48 = 123  (tick = cc*120) |
| 5880 | `b0 31 20` | CC#49 = 32  (tick = cc*120) |
| 6000 | `b0 32 45` | CC#50 = 69  (tick = cc*120) |
| 6120 | `b0 33 6a` | CC#51 = 106  (tick = cc*120) |
| 6240 | `b0 34 0f` | CC#52 = 15  (tick = cc*120) |
| 6360 | `b0 35 34` | CC#53 = 52  (tick = cc*120) |
| 6480 | `b0 36 59` | CC#54 = 89  (tick = cc*120) |
| 6600 | `b0 37 7e` | CC#55 = 126  (tick = cc*120) |
| 6720 | `b0 38 23` | CC#56 = 35  (tick = cc*120) |
| 6840 | `b0 39 48` | CC#57 = 72  (tick = cc*120) |
| 6960 | `b0 3a 6d` | CC#58 = 109  (tick = cc*120) |
| 7080 | `b0 3b 12` | CC#59 = 18  (tick = cc*120) |
| 7200 | `b0 3c 37` | CC#60 = 55  (tick = cc*120) |
| 7320 | `b0 3d 5c` | CC#61 = 92  (tick = cc*120) |
| 7440 | `b0 3e 01` | CC#62 = 1  (tick = cc*120) |
| 7560 | `b0 3f 26` | CC#63 = 38  (tick = cc*120) |
| 7680 | `b0 40 4b` | CC#64 = 75  (tick = cc*120) |
| 7800 | `b0 41 70` | CC#65 = 112  (tick = cc*120) |
| 7920 | `b0 42 15` | CC#66 = 21  (tick = cc*120) |
| 8040 | `b0 43 3a` | CC#67 = 58  (tick = cc*120) |
| 8160 | `b0 44 5f` | CC#68 = 95  (tick = cc*120) |
| 8280 | `b0 45 04` | CC#69 = 4  (tick = cc*120) |
| 8400 | `b0 46 29` | CC#70 = 41  (tick = cc*120) |
| 8520 | `b0 47 4e` | CC#71 = 78  (tick = cc*120) |
| 8640 | `b0 48 73` | CC#72 = 115  (tick = cc*120) |
| 8760 | `b0 49 18` | CC#73 = 24  (tick = cc*120) |
| 8880 | `b0 4a 3d` | CC#74 = 61  (tick = cc*120) |
| 9000 | `b0 4b 62` | CC#75 = 98  (tick = cc*120) |
| 9120 | `b0 4c 07` | CC#76 = 7  (tick = cc*120) |
| 9240 | `b0 4d 2c` | CC#77 = 44  (tick = cc*120) |
| 9360 | `b0 4e 51` | CC#78 = 81  (tick = cc*120) |
| 9480 | `b0 4f 76` | CC#79 = 118  (tick = cc*120) |
| 9600 | `b0 50 1b` | CC#80 = 27  (tick = cc*120) |
| 9720 | `b0 51 40` | CC#81 = 64  (tick = cc*120) |
| 9840 | `b0 52 65` | CC#82 = 101  (tick = cc*120) |
| 9960 | `b0 53 0a` | CC#83 = 10  (tick = cc*120) |
| 10080 | `b0 54 2f` | CC#84 = 47  (tick = cc*120) |
| 10200 | `b0 55 54` | CC#85 = 84  (tick = cc*120) |
| 10320 | `b0 56 79` | CC#86 = 121  (tick = cc*120) |
| 10440 | `b0 57 1e` | CC#87 = 30  (tick = cc*120) |
| 10560 | `b0 58 43` | CC#88 = 67  (tick = cc*120) |
| 10680 | `b0 59 68` | CC#89 = 104  (tick = cc*120) |
| 10800 | `b0 5a 0d` | CC#90 = 13  (tick = cc*120) |
| 10920 | `b0 5b 32` | CC#91 = 50  (tick = cc*120) |
| 11040 | `b0 5c 57` | CC#92 = 87  (tick = cc*120) |
| 11160 | `b0 5d 7c` | CC#93 = 124  (tick = cc*120) |
| 11280 | `b0 5e 21` | CC#94 = 33  (tick = cc*120) |
| 11400 | `b0 5f 46` | CC#95 = 70  (tick = cc*120) |
| 12240 | `b0 66 49` | CC#102 = 73  (tick = cc*120) |
| 12360 | `b0 67 6e` | CC#103 = 110  (tick = cc*120) |
| 12480 | `b0 68 13` | CC#104 = 19  (tick = cc*120) |
| 12600 | `b0 69 38` | CC#105 = 56  (tick = cc*120) |
| 12720 | `b0 6a 5d` | CC#106 = 93  (tick = cc*120) |
| 12840 | `b0 6b 02` | CC#107 = 2  (tick = cc*120) |
| 12960 | `b0 6c 27` | CC#108 = 39  (tick = cc*120) |
| 13080 | `b0 6d 4c` | CC#109 = 76  (tick = cc*120) |
| 13200 | `b0 6e 71` | CC#110 = 113  (tick = cc*120) |
| 13320 | `b0 6f 16` | CC#111 = 22  (tick = cc*120) |
| 13440 | `b0 70 3b` | CC#112 = 59  (tick = cc*120) |
| 13560 | `b0 71 60` | CC#113 = 96  (tick = cc*120) |
| 13680 | `b0 72 05` | CC#114 = 5  (tick = cc*120) |
| 13800 | `b0 73 2a` | CC#115 = 42  (tick = cc*120) |
| 13920 | `b0 74 4f` | CC#116 = 79  (tick = cc*120) |
| 14040 | `b0 75 74` | CC#117 = 116  (tick = cc*120) |
| 14160 | `b0 76 19` | CC#118 = 25  (tick = cc*120) |
| 14280 | `b0 77 3e` | CC#119 = 62  (tick = cc*120) |
| 14400 | `b0 78 63` | CC#120 = 99  (tick = cc*120) |
| 14520 | `b0 79 08` | CC#121 = 8  (tick = cc*120) |
| 14640 | `b0 7a 2d` | CC#122 = 45  (tick = cc*120) |
| 14760 | `b0 7b 52` | CC#123 = 82  (tick = cc*120) |
| 14880 | `b0 7c 77` | CC#124 = 119  (tick = cc*120) |
| 15000 | `b0 7d 1c` | CC#125 = 28  (tick = cc*120) |
| 15120 | `b0 7e 41` | CC#126 = 65  (tick = cc*120) |
| 15240 | `b0 7f 66` | CC#127 = 102  (tick = cc*120) |
| 15360 | `b0 0b 00` | CC#11 = 0  (range test) |
| 15480 | `b0 0b 01` | CC#11 = 1  (range test) |
| 15600 | `b0 0b 3f` | CC#11 = 63  (range test) |
| 15720 | `b0 0b 40` | CC#11 = 64  (range test) |
| 15840 | `b0 0b 7e` | CC#11 = 126  (range test) |
| 15960 | `b0 0b 7f` | CC#11 = 127  (range test) |

## Track 3: Bank Program ch2  · channel 2 (0-based 1)

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 10 42 61 6e 6b 20 50 72 6f 67 72 61 6d 20 63 68 32` | track name 'Bank Program ch2' |
| 0 | `c1 05` | PROGRAM 5 (0-based; Domino may show 6) |
| 480 | `b1 00 12` | CC#0 = 18  bank MSB |
| 480 | `b1 20 34` | CC#32 = 52  bank LSB |
| 480 | `c1 56` | PROGRAM 86 (0-based; Domino may show 87) |
| 960 | `b1 00 21` | CC#0 = 33  bank MSB only |
| 960 | `c1 07` | PROGRAM 7 (0-based; Domino may show 8) |
| 1440 | `b1 20 43` | CC#32 = 67  bank LSB only |
| 1440 | `c1 08` | PROGRAM 8 (0-based; Domino may show 9) |
| 1920 | `c1 00` | PROGRAM 0 (0-based; Domino may show 1) |
| 2400 | `c1 7f` | PROGRAM 127 (0-based; Domino may show 128) |
| 2880 | `b1 00 79` | CC#0 = 121  bank MSB alone, no program |
| 3360 | `b1 20 0a` | CC#32 = 10  bank LSB alone, no program |
| 3840 | `b1 00 01` | CC#0 = 1  bank MSB, separate ticks |
| 3900 | `b1 20 02` | CC#32 = 2  bank LSB, separate ticks |
| 3960 | `c1 03` | PROGRAM 3 (0-based; Domino may show 4) |

## Track 4: RPN NRPN ch3  · channel 3 (0-based 2)

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 0c 52 50 4e 20 4e 52 50 4e 20 63 68 33` | track name 'RPN NRPN ch3' |
| 0 | `b2 65 00` | CC#101 = 0  [RPN 0/0 pitch-bend sens = 12/0] |
| 0 | `b2 64 00` | CC#100 = 0  [RPN 0/0 pitch-bend sens = 12/0] |
| 0 | `b2 06 0c` | CC#6 = 12  [RPN 0/0 pitch-bend sens = 12/0] |
| 0 | `b2 26 00` | CC#38 = 0  [RPN 0/0 pitch-bend sens = 12/0] |
| 480 | `b2 65 00` | CC#101 = 0  [RPN 0/1 fine tune 64/1] |
| 480 | `b2 64 01` | CC#100 = 1  [RPN 0/1 fine tune 64/1] |
| 480 | `b2 06 40` | CC#6 = 64  [RPN 0/1 fine tune 64/1] |
| 480 | `b2 26 01` | CC#38 = 1  [RPN 0/1 fine tune 64/1] |
| 960 | `b2 65 00` | CC#101 = 0  [RPN 0/2 coarse tune 70, MSB only] |
| 960 | `b2 64 02` | CC#100 = 2  [RPN 0/2 coarse tune 70, MSB only] |
| 960 | `b2 06 46` | CC#6 = 70  [RPN 0/2 coarse tune 70, MSB only] |
| 1440 | `b2 63 01` | CC#99 = 1  [NRPN 1/8 = 80, MSB only] |
| 1440 | `b2 62 08` | CC#98 = 8  [NRPN 1/8 = 80, MSB only] |
| 1440 | `b2 06 50` | CC#6 = 80  [NRPN 1/8 = 80, MSB only] |
| 1920 | `b2 63 12` | CC#99 = 18  [NRPN 18/52 = 86/120] |
| 1920 | `b2 62 34` | CC#98 = 52  [NRPN 18/52 = 86/120] |
| 1920 | `b2 06 56` | CC#6 = 86  [NRPN 18/52 = 86/120] |
| 1920 | `b2 26 78` | CC#38 = 120  [NRPN 18/52 = 86/120] |
| 2400 | `b2 65 7f` | CC#101 = 127  [RPN null] |
| 2400 | `b2 64 7f` | CC#100 = 127  [RPN null] |
| 2880 | `b2 65 00` | CC#101 = 0  [RPN 0/0 + data increment] |
| 2880 | `b2 64 00` | CC#100 = 0  [RPN 0/0 + data increment] |
| 2880 | `b2 60 00` | CC#96 = 0  [RPN 0/0 + data increment] |
| 3360 | `b2 61 00` | CC#97 = 0  [data decrement alone] |
| 3840 | `b2 06 33` | CC#6 = 51  [data entry MSB alone] |
| 4320 | `b2 26 44` | CC#38 = 68  [data entry LSB alone] |
| 4800 | `b2 65 05` | CC#101 = 5  [RPN MSB alone] |
| 5280 | `b2 64 06` | CC#100 = 6  [RPN LSB alone] |
| 5760 | `b2 63 07` | CC#99 = 7  [NRPN MSB alone] |
| 6240 | `b2 62 08` | CC#98 = 8  [NRPN LSB alone] |

## Track 5: Pitch bend ch4  · channel 4 (0-based 3)

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 0e 50 69 74 63 68 20 62 65 6e 64 20 63 68 34` | track name 'Pitch bend ch4' |
| 0 | `e3 00 00` | PITCH BEND raw=0 (signed -8192) |
| 480 | `e3 00 40` | PITCH BEND raw=8192 (signed 0) |
| 960 | `e3 7f 7f` | PITCH BEND raw=16383 (signed 8191) |
| 1440 | `e3 01 00` | PITCH BEND raw=1 (signed -8191) |
| 1920 | `e3 34 24` | PITCH BEND raw=4660 (signed -3532) |
| 2400 | `e3 01 40` | PITCH BEND raw=8193 (signed 1) |

## Track 6: Aftertouch ch5  · channel 5 (0-based 4)

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 0e 41 66 74 65 72 74 6f 75 63 68 20 63 68 35` | track name 'Aftertouch ch5' |
| 0 | `94 3c 64` | NOTE key=60 vel=100 gate=1920 (off vel 64) |
| 0 | `94 3d 64` | NOTE key=61 vel=100 gate=1920 (off vel 64) |
| 0 | `94 7f 64` | NOTE key=127 vel=100 gate=1920 (off vel 64) |
| 120 | `a4 3c 63` | POLY PRESSURE key=60 val=99 |
| 240 | `a4 3d 00` | POLY PRESSURE key=61 val=0 |
| 360 | `a4 7f 7f` | POLY PRESSURE key=127 val=127 |
| 480 | `a4 3c 01` | POLY PRESSURE key=60 val=1 |
| 960 | `d4 00` | CHANNEL PRESSURE 0 |
| 1080 | `d4 40` | CHANNEL PRESSURE 64 |
| 1200 | `d4 7f` | CHANNEL PRESSURE 127 |
| 1320 | `d4 21` | CHANNEL PRESSURE 33 |

## Track 7: Drums ch10  · channel 10 (0-based 9)

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 0a 44 72 75 6d 73 20 63 68 31 30` | track name 'Drums ch10' |
| 0 | `ff 04 07 44 72 75 6d 4b 69 74` | INSTRUMENT NAME 'DrumKit' |
| 0 | `99 24 6e` | NOTE key=36 vel=110 gate=120 (off vel 64) |
| 240 | `99 26 6d` | NOTE key=38 vel=109 gate=120 (off vel 64) |
| 480 | `99 2a 6c` | NOTE key=42 vel=108 gate=120 (off vel 64) |

## Track 8: Ch16 Port2  · channel 16 (0-based 15)

| tick | raw bytes | event |
|---:|---|---|
| 0 | `ff 03 0a 43 68 31 36 20 50 6f 72 74 32` | track name 'Ch16 Port2' |
| 0 | `ff 21 01 01` | MIDI PORT prefix = 1 (second port) |
| 0 | `ff 20 01 0f` | MIDI CHANNEL prefix = 15 |
| 0 | `9f 48 40` | NOTE key=72 vel=64 gate=480 (off vel 64) |
