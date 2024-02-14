// use phf::phf_map;

// pub struct Emojis(phf::Map<&'static str, &'static str>);

// impl Emojis {
//     pub fn emojis(&self, name: &str) -> Vec<&'static str> {
//         let keys = self
//             .0
//             .keys()
//             .filter(|k| k.starts_with(name))
//             .collect::<Vec<&&str>>();
//         keys.iter().filter_map(|k| self.0.get(k)).copied().collect()
//     }
// }

// pub const EMOJIS: Emojis = Emojis(phf_map!(
// "acid" => "⊂(◉‿◉)つ",
// "afraid" => "(ㆆ _ ㆆ)",
// "alpha" => "α",
// "angel" => "☜(⌒▽⌒)☞",
// "angry" => "•`_´•",
// "arrowhead"  => "⤜(ⱺ ʖ̯ⱺ)⤏",
// "apple" => "",
// "ass" =>  "(‿|‿)",
// "awkward" => "•͡˘㇁•͡˘",
// "bat" => r#"/|\ ^._.^ /|\"#,
// "bear" => "ʕ·͡ᴥ·ʔ",
// "bearflip" => "ʕノ•ᴥ•ʔノ ︵ ┻━┻",
// "bearhug" => "ʕっ•ᴥ•ʔっ",
// "beta" => "β",
// "bigheart" => "❤",
// "blackeye" => "0__#",
// "blubby" => "(      0    _   0    )",
// "blush" => "(˵ ͡° ͜ʖ ͡°˵)",
// "bond" => "┌( ͝° ͜ʖ͡°)=ε/̵͇̿̿/’̿’̿ ̿",
// "boobs" => "( . Y . )",
// "bored" => "(-_-)",
// "bribe" => "( •͡˘ _•͡˘)ノð",
// "bubbles" => "( ˘ ³˘)ノ°ﾟº❍｡",
// "butterfly "  => "ƸӜƷ",
// "cat" => "(= ФェФ=)",
// "catlenny" => "( ͡° ᴥ ͡°)",
// "checkmark" => "✔",
// "cheer" => r#"※\(^o^)/※"#,
// "chubby" => "╭(ʘ̆~◞౪◟~ʘ̆)╮",
// "claro" => "(͡ ° ͜ʖ ͡ °)",
// "clique" => "ヽ༼ ຈل͜ຈ༼ ▀̿̿Ĺ̯̿̿▀̿ ̿༽Ɵ͆ل͜Ɵ͆ ༽ﾉ",
// "cloud" => "☁",
// "club" => "♣",
// "coffee" => "c[_]",
// "cmd" => "⌘",
// "cool" => "(•_•) ( •_•)>⌐■-■ (⌐■_■)",
// "copyright" => "©",
// "creep" => "ԅ(≖‿≖ԅ)",
// "creepcute"  => "ƪ(ړײ)ƪ",
// "crim3s" => "( ✜︵✜ )",
// "cross" => "†",
// "cry"  => "(╥﹏╥)",
// "crywave" => "( ╥﹏╥) ノシ",
// "cute" => "(｡◕‿‿◕｡)",
// "d1" => "⚀",
// "d2" => "⚁",
// "d3" => "⚂",
// "d4" => "⚃",
// "d5" => "⚄",
// "d6" => "⚅",
// "dab" => "ヽ( •_)ᕗ",
// "damnyou" => "(ᕗ ͠° ਊ ͠° )ᕗ",
// "dance" => "ᕕ(⌐■_■)ᕗ ♪♬",
// "dead" => "x⸑x",
// "dealwithit" => "(⌐■_■)",
// "delta" => "Δ",
// "depressed" => "(︶︹︶)",
// "derp" => "☉ ‿ ⚆",
// "diamond" => "♦",
// "dj" => "d[-_-]b",
// "dog" => "(◕ᴥ◕ʋ)",
// "dollar" => "$",
// "dong" => "(̿▀̿ ̿Ĺ̯̿̿▀̿ ̿)̄",
// "donger" => "ヽ༼ຈل͜ຈ༽ﾉ",
// "dontcare" => "(- ʖ̯-)",
// "donotwant" => "ヽ(｀Д´)ﾉ",
// "dope" => "<(^_^)>",
// "<<" => "«",
// ">>" => "»",
// "doubleflat" => "𝄫",
// "doublesharp" => "𝄪",
// "doubletableflip" => "┻━┻ ︵ヽ(`Д´)ﾉ︵ ┻━┻",
// "down" => "↓",
// "duckface" => "(・3・)",
// "duel" => "ᕕ(╭ರ╭ ͟ʖ╮•́)⊃¤=(————-",
// "duh" => "(≧︿≦)",
// "dunnolol" => r#"¯\(°_o)/¯"#,
// "eeriemob" => "(-(-_-(-_(-_(-_-)_-)-_-)_-)_-)-)",
// "..." => "…",
// "-"  => "–",
// "emptystar" => "☆",
// "triangleempty" => "△",
// "endure" => "(҂◡_◡) ᕤ",
// "letter" => "✉︎",
// "epsilon" => "ɛ",
// "euro" => "€",
// "evil" => "ψ(｀∇´)ψ",
// "evillenny" => "(͠≖ ͜ʖ͠≖)",
// "excited" => "(ﾉ◕ヮ◕)ﾉ*:・ﾟ✧",
// "execution" => "(⌐■_■)︻╦╤─   (╥﹏╥)",
// "facepalm" => "(－‸ლ)",
// "fart" => "(ˆ⺫ˆ๑)<3",
// "fight" => "(ง •̀_•́)ง",
// "finn" => "| (• ◡•)|",
// "fish" => r#"<"(((<3"#,
// "5" => "卌",
// "5/8" => "⅝",
// "flat" => "♭",
// "flexing" => "ᕙ(`▽´)ᕗ",
// "heavytable" => r#"┬─┬﻿ ︵ /(.□. \）"#,
// "flower"  => "(✿◠‿◠)",
// "flower2" => "✿",
// "fly" => "─=≡Σ((( つ◕ل͜◕)つ",
// "friendflip" => "(╯°□°)╯︵ ┻━┻ ︵ ╯(°□° ╯)",
// "frown" => "(ღ˘⌣˘ღ)",
// "gtfo" => "୧༼ಠ益ಠ╭∩╮༽",
// "fu" => "┌П┐(ಠ_ಠ)",
// "sir" => "ಠ_ರೃ",
// "ghast" => "= _ =",
// "ghost"  => "༼ つ ╹ ╹ ༽つ",
// "gift" => "(´・ω・)っ由",
// "gimme" => "༼ つ ◕_◕ ༽つ",
// "givemeyourmoney" => "(•-•)⌐",
// "glitter" => "(*・‿・)ノ⌒*:･ﾟ✧",
// "glasses" => "(⌐ ͡■ ͜ʖ ͡■)",
// "glassesoff" => "( ͡° ͜ʖ ͡°)ﾉ⌐■-■",
// "glitterderp" => "(ﾉ☉ヮ⚆)ﾉ ⌒*:･ﾟ✧",
// "gloomy" => "(_゜_゜_)",
// "goatse" => "(з๏ε)",
// "gotit" => "(☞ﾟ∀ﾟ)☞",
// "greet" => "( ´◔ ω◔`) ノシ",
// "hadouken" => "༼つಠ益ಠ༽つ ─=≡ΣO))",
// "hammerandsickle"  => "☭",
// "hs" => "☭",
// "soviet" => "☭",
// "handleft" => "☜",
// "handright" => "☞",
// "haha" => "٩(^‿^)۶",
// "happy" => "٩( ๑╹ ꇴ╹)۶",
// "happygarry" => "ᕕ( ᐛ )ᕗ",
// "heart" => "♥",
// "hello" => "(ʘ‿ʘ)╯",
// "ohai"  => "(ʘ‿ʘ)╯",
// "bye" => "(ʘ‿ʘ)╯",
// "help" => r#"\(°Ω°)/"#,
// "highfive" => r#"._.)/\(._."#,
// "hitting" => r#"( ｀皿´)｡ﾐ/"#,
// "hugs" => "(づ｡◕‿‿◕｡)づ",
// "iknowright" => "┐｜･ิω･ิ#｜┌",
// "illuminati" => "୧(▲ᴗ▲)ノ",
// "inf" => "∞",
// "inlove" => "(っ´ω`c)♡",
// "int" => "∫",
// "internet" => "ଘ(੭*ˊᵕˋ)੭* ̀ˋ ɪɴᴛᴇʀɴᴇᴛ",
// "interrobang" => "‽",
// "jake" => "(❍ᴥ❍ʋ)",
// "kappa" => "(¬,‿,¬)",
// "kawaii" => "≧◡≦",
// "keen" => "┬┴┬┴┤Ɵ͆ل͜Ɵ͆ ༽ﾉ",
// "kiahh" => r#"~\(≧▽≦)/~"#,
// "kiss" => "(づ ￣ ³￣)づ",
// "kyubey" => "／人◕ ‿‿ ◕人＼",
// "lambda" => "λ",
// "lazy" => "_(:3」∠)_",
// "left" => "←",
// "lenny" => "( ͡° ͜ʖ ͡°)",
// "lennybill" => "[̲̅$̲̅(̲̅ ͡° ͜ʖ ͡°̲̅)̲̅$̲̅]",
// "lennyfight" => "(ง ͠° ͟ʖ ͡°)ง",
// "lennyflip" => "(ノ ͡° ͜ʖ ͡°ノ)   ︵ ( ͜。 ͡ʖ ͜。)",
// "lennygang" => "( ͡°( ͡° ͜ʖ( ͡° ͜ʖ ͡°)ʖ ͡°) ͡°)",
// "lennyshrug"  => r#"¯\_( ͡° ͜ʖ ͡°)_/¯"#,
// "lennysir" => "( ಠ ͜ʖ ರೃ)",
// "lennystalker" => "┬┴┬┴┤( ͡° ͜ʖ├┬┴┬┴",
// "lennystrong" => "ᕦ( ͡° ͜ʖ ͡°)ᕤ",
// "lennywizard" => "╰( ͡° ͜ʖ ͡° )つ──☆*:・ﾟ",
// "lol" => "L(° O °L)",
// "look" => "(ಡ_ಡ)☞",
// "loud" => "ᕦ(⩾﹏⩽)ᕥ",
// "noise" => "ᕦ(⩾﹏⩽)ᕥ",
// "love" => "♥‿♥",
// "lovebear" => "ʕ♥ᴥ♥ʔ",
// "lumpy" => "꒰ ꒡⌓꒡꒱",
// "luv" => "-`ღ´-",
// "magic" => "ヽ(｀Д´)⊃━☆ﾟ. * ･ ｡ﾟ,",
// "magicflip" => "(/¯◡ ‿ ◡)/¯ ~ ┻━┻",
// "meep" => r#"\(°^°)/"#,
// "meh" => "ಠ_ಠ",
// "metal" => r#"\m/,(> . <)_\m/"#,
// "rock" => r#"\m/,(> . <)_\m/"#,
// "mistyeyes" => "ಡ_ಡ",
// "monster" => "༼ ༎ຶ ෴ ༎ຶ༽",
// "natural" => "♮",
// "nerd" => "(⌐⊙_⊙)",
// "nice" => "( ͡° ͜ °)",
// "no" => "→_←",
// "noclue" => "／人◕ __ ◕人＼",
// "nom"  => "(っˆڡˆς)",
// "yummy"  => "(っˆڡˆς)",
// "delicious" => "(っˆڡˆς)",
// "note" => "♫",
// "sing" => "♫",
// "radioactive" => "☢",
// "nyan" => "~=[,,_,,]:3",
// "nyeh" => "@^@",
// "ohshit" => "( º﹃º )",
// "omega" => "Ω",
// "omg" => "◕_◕",
// "1/8" => "⅛",
// "1/4" => "¼",
// "1/2" => "½",
// "1/3" => "⅓",
// "opt" => r#"⌥""#,
// "orly" => "(눈_눈)",
// "ohyou" => "(◞థ౪థ)ᴖ",
// "peace" => "✌(-‿-)✌",
// "pear" => "(__>-",
// "pi" => "π",
// "pingpong" => "( •_•)O*¯`·.¸.·´¯`°Q(•_• )",
// "plain" => "._.",
// "pleased" => "(˶‾᷄ ⁻̫ ‾᷅˵)",
// "point" => "(☞ﾟヮﾟ)☞",
// "pooh" => "ʕ •́؈•̀)",
// "porcupine" => "(•ᴥ• )́`́'́`́'́⻍",
// "pound" => "£",
// "praise" => "(☝ ՞ਊ ՞)☝",
// "punch" => "O=('-'Q)",
// "rage" => "t(ಠ益ಠt)",
// "rageflip" => "(ノಠ益ಠ)ノ彡┻━┻",
// "rainbowcat" => "(=^･ｪ･^=))ﾉ彡☆",
// "really" => "ò_ô",
// "registered" => "®",
// "right" => "→",
// "riot" => "୧༼ಠ益ಠ༽୨",
// "rolleyes" => "(◔_◔)",
// "rose" => "✿ڿڰۣ—",
// "run" => "(╯°□°)╯",
// "sad" => "ε(´סּ︵סּ`)з",
// "saddonger" => "ヽ༼ຈʖ̯ຈ༽ﾉ",
// "sadlenny" => "( ͡° ʖ̯ ͡°)",
// "7/8" => "⅞",
// "sharp" => "♯",
// "shout" => "╚(•⌂•)╝",
// "shrug" => r#"¯\_(ツ)_/¯"#,
// "shy" => "=^_^=",
// "sigma" => "Σ",
// "skull" => "☠",
// "smile" => "ツ",
// "smiley" => "☺︎",
// "smirk" => "¬‿¬",
// "snowman" => "☃",
// "sob" => "(;´༎ຶД༎ຶ`)",
// "soviettableflip"  => r#"ノ┬─┬ノ ︵ ( \o°o)\"#,
// "spade" => "♠",
// "sqrt" => "√",
// "squid" => "<コ:彡",
// "star" => "★",
// "strong" => "ᕙ(⇀‸↼‶)ᕗ",
// "sum" => "∑",
// "sun" => "☀",
// "surprised" => "(๑•́ ヮ •̀๑)",
// "surrender" => r#"\_(-_-)_/"#,
// "stalker" => "┬┴┬┴┤(･_├┬┴┬┴",
// "swag" => "(̿▀̿‿ ̿▀̿ ̿)",
// "sword" => "o()xxxx[{::::::::::::::::::>",
// "tabledown" => r#"┬─┬﻿ ノ( ゜-゜ノ)"#,
// "tableflip" => r#"(ノ ゜Д゜)ノ ︵ ┻━┻"#,
// "tau" => "τ",
// "tears" => "(ಥ﹏ಥ)",
// "terrorist" => "୧༼ಠ益ಠ༽︻╦╤─",
// "thanks" => r#"\(^-^)/"#,
// "therefore" => "⸫",
// "this" => "( ͡° ͜ʖ ͡°)_/¯",
// "tiefighter" =>  "|=-(¤)-=|",
// "tired" => "(=____=)",
// "toldyou" => "☜(꒡⌓꒡)",
// "toogood" => "ᕦ(òᴥó)ᕥ",
// "trademark" => "™",
// "triangle" => "▲",
// "2/3" => "⅔",
// "unflip" => "┬──┬ ノ(ò_óノ)",
// "up" => "↑",
// "victory" => "(๑•̀ㅂ•́)ง✧",
// "wat" => "(ÒДÓױ)",
// "wave" => "( * ^ *) ノシ",
// "whaa" => "Ö",
// "whistle" => "(っ^з^)♪♬",
// "whoa" => "(°o•)",
// "why" => "ლ(`◉◞౪◟◉‵ლ)",
// "woo" => "＼(＾O＾)／",
// "wtf" => "(⊙＿⊙')",
// "wut" => "⊙ω⊙",
// "yay" => r#"\( ﾟヮﾟ)/"#,
// "yeah" => "(•̀ᴗ•́)و",
// "yen" => "¥",
// "yinyang" => "☯",
// "yolo" => "Yᵒᵘ Oᶰˡʸ Lᶤᵛᵉ Oᶰᶜᵉ",
// "youkids" => "ლ༼>╭ ͟ʖ╮<༽ლ",
// "yuno" => "(屮ﾟДﾟ)屮 Y U NO",
// "zen" => "⊹╰(⌣ʟ⌣)╯⊹",
// "meditation" => "⊹╰(⌣ʟ⌣)╯⊹",
// "omm" => "⊹╰(⌣ʟ⌣)╯⊹",
// "zoidberg" => "(V) (°,,,,°) (V)",
// "zombie" => "[¬º-°]¬",
// ));