//! Built-in catalog of generic fixture profiles (no brand names).
//! Every profile can be duplicated and edited as a custom fixture.

use crate::model::ChannelKind::*;
use crate::model::*;

fn wheel() -> Vec<ColorSlot> {
    let c = |value: u8, color: [u8; 3]| ColorSlot { value, color };
    vec![
        c(0, [255, 255, 255]),
        c(10, [255, 30, 30]),
        c(20, [30, 255, 60]),
        c(30, [40, 60, 255]),
        c(40, [255, 230, 40]),
        c(50, [255, 140, 20]),
        c(60, [40, 230, 255]),
        c(70, [255, 40, 220]),
    ]
}

#[allow(clippy::too_many_arguments)]
fn def(id: &str, name: &str, kind: FixtureKind, channels: Vec<ChannelDef>, beam: f32) -> FixtureDef {
    FixtureDef {
        id: id.to_string(),
        name: name.to_string(),
        kind,
        channels,
        pan_range: 540.0,
        tilt_range: 270.0,
        color_wheel: vec![],
        beam_angle: beam,
        shutter_open: 255,
        shutter_closed: 0,
        strobe_min: 10,
        strobe_max: 250,
        custom: false,
    }
}

pub fn builtin() -> Vec<FixtureDef> {
    let mut v = vec![
        def(
            "par-rgb-3",
            "PAR LED RGB 3 canali",
            FixtureKind::Par,
            vec![ch("Rosso", Red, 0), ch("Verde", Green, 0), ch("Blu", Blue, 0)],
            30.0,
        ),
        def(
            "par-rgb-7",
            "PAR LED RGB 7 canali",
            FixtureKind::Par,
            vec![
                ch("Dimmer", Dimmer, 0),
                ch("Rosso", Red, 0),
                ch("Verde", Green, 0),
                ch("Blu", Blue, 0),
                ch("Strobo", Strobe, 0),
                ch("Modalità", Fixed, 0),
                ch("Velocità programmi", Other, 0),
            ],
            30.0,
        ),
        def(
            "par-rgbw-8",
            "PAR LED RGBW 8 canali",
            FixtureKind::Par,
            vec![
                ch("Dimmer", Dimmer, 0),
                ch("Rosso", Red, 0),
                ch("Verde", Green, 0),
                ch("Blu", Blue, 0),
                ch("Bianco", White, 0),
                ch("Strobo", Strobe, 0),
                ch("Modalità", Fixed, 0),
                ch("Velocità programmi", Other, 0),
            ],
            30.0,
        ),
        def(
            "par-6in1-10",
            "PAR LED RGBWA+UV 10 canali",
            FixtureKind::Par,
            vec![
                ch("Dimmer", Dimmer, 0),
                ch("Rosso", Red, 0),
                ch("Verde", Green, 0),
                ch("Blu", Blue, 0),
                ch("Bianco", White, 0),
                ch("Ambra", Amber, 0),
                ch("UV", Uv, 0),
                ch("Strobo", Strobe, 0),
                ch("Modalità", Fixed, 0),
                ch("Velocità programmi", Other, 0),
            ],
            25.0,
        ),
        def(
            "blinder-2",
            "Accecatore 2 celle (2 canali)",
            FixtureKind::Blinder,
            vec![ch("Cella 1", White, 0), ch("Cella 2", White, 0)],
            60.0,
        ),
        def(
            "blinder-4",
            "Accecatore 4 celle (4 canali)",
            FixtureKind::Blinder,
            vec![ch("Cella 1", White, 0), ch("Cella 2", White, 0), ch("Cella 3", White, 0), ch("Cella 4", White, 0)],
            60.0,
        ),
        def("blinder-cob-1", "Accecatore LED COB (1 canale)", FixtureKind::Blinder, vec![ch("Intensità", Dimmer, 0)], 60.0),
        def(
            "strobe-2",
            "Strobo LED (2 canali)",
            FixtureKind::Strobe,
            vec![ch("Dimmer", Dimmer, 0), ch("Velocità strobo", Strobe, 0)],
            90.0,
        ),
        def(
            "bar-rgb-4",
            "Barra LED RGB (4 canali)",
            FixtureKind::LedBar,
            vec![ch("Dimmer", Dimmer, 0), ch("Rosso", Red, 0), ch("Verde", Green, 0), ch("Blu", Blue, 0)],
            40.0,
        ),
        def("dimmer-1", "Canale dimmer generico (1 canale)", FixtureKind::Other, vec![ch("Dimmer", Dimmer, 0)], 40.0),
    ];

    let mut spot9 = def(
        "head-spot-9",
        "Testa mobile Spot 9 canali",
        FixtureKind::MovingSpot,
        vec![
            ch("Pan", Pan, 128),
            ch("Tilt", Tilt, 128),
            ch("Colore", ColorWheel, 0),
            ch("Gobo", Gobo, 0),
            ch("Strobo", Shutter, 0),
            ch("Dimmer", Dimmer, 0),
            ch("Velocità P/T", PanTiltSpeed, 0),
            ch("Auto", Fixed, 0),
            ch("Reset", Fixed, 0),
        ],
        14.0,
    );
    spot9.color_wheel = wheel();
    spot9.shutter_open = 0;
    spot9.strobe_min = 10;
    spot9.strobe_max = 255;
    v.push(spot9);

    let mut spot11 = def(
        "head-spot-11",
        "Testa mobile Spot 11 canali (16 bit)",
        FixtureKind::MovingSpot,
        vec![
            ch("Pan", Pan, 128),
            ch("Pan fine", PanFine, 0),
            ch("Tilt", Tilt, 128),
            ch("Tilt fine", TiltFine, 0),
            ch("Velocità P/T", PanTiltSpeed, 0),
            ch("Dimmer", Dimmer, 0),
            ch("Shutter", Shutter, 255),
            ch("Colore", ColorWheel, 0),
            ch("Gobo", Gobo, 0),
            ch("Auto", Fixed, 0),
            ch("Reset", Fixed, 0),
        ],
        14.0,
    );
    spot11.color_wheel = wheel();
    spot11.shutter_open = 255;
    spot11.strobe_min = 10;
    spot11.strobe_max = 250;
    v.push(spot11);

    let mut beam = def(
        "head-beam-16",
        "Testa mobile Beam 16 canali",
        FixtureKind::MovingBeam,
        vec![
            ch("Colore", ColorWheel, 0),
            ch("Shutter", Shutter, 4),
            ch("Dimmer", Dimmer, 0),
            ch("Gobo", Gobo, 0),
            ch("Prisma", Prism, 0),
            ch("Rotazione prisma", Other, 0),
            ch("Frost", Other, 0),
            ch("Fuoco", Focus, 128),
            ch("Pan", Pan, 128),
            ch("Pan fine", PanFine, 0),
            ch("Tilt", Tilt, 128),
            ch("Tilt fine", TiltFine, 0),
            ch("Velocità P/T", PanTiltSpeed, 0),
            ch("Reset", Fixed, 0),
            ch("Lampada", Fixed, 0),
            ch("Macro", Other, 0),
        ],
        4.0,
    );
    beam.color_wheel = wheel();
    beam.shutter_open = 4;
    beam.strobe_min = 8;
    beam.strobe_max = 215;
    v.push(beam);

    let mut wash13 = def(
        "head-wash-13",
        "Testa mobile Wash RGBW 13 canali",
        FixtureKind::MovingWash,
        vec![
            ch("Pan", Pan, 128),
            ch("Pan fine", PanFine, 0),
            ch("Tilt", Tilt, 128),
            ch("Tilt fine", TiltFine, 0),
            ch("Velocità P/T", PanTiltSpeed, 0),
            ch("Dimmer", Dimmer, 0),
            ch("Strobo", Strobe, 0),
            ch("Rosso", Red, 0),
            ch("Verde", Green, 0),
            ch("Blu", Blue, 0),
            ch("Bianco", White, 0),
            ch("Auto", Fixed, 0),
            ch("Reset", Fixed, 0),
        ],
        22.0,
    );
    wash13.strobe_min = 10;
    v.push(wash13);

    let wash9 = def(
        "head-wash-9",
        "Mini testa mobile Wash RGBW 9 canali",
        FixtureKind::MovingWash,
        vec![
            ch("Pan", Pan, 128),
            ch("Tilt", Tilt, 128),
            ch("Velocità P/T", PanTiltSpeed, 0),
            ch("Dimmer", Dimmer, 0),
            ch("Strobo", Strobe, 0),
            ch("Rosso", Red, 0),
            ch("Verde", Green, 0),
            ch("Blu", Blue, 0),
            ch("Bianco", White, 0),
        ],
        20.0,
    );
    v.push(wash9);
    v
}
