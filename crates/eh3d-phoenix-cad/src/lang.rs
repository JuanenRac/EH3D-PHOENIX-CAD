//! The words of the window in the eight languages of the repository (English, Spanish, German,
//! French, Italian, Japanese, Chinese and Portuguese of Portugal).
//!
//! The diagnostics of the library have their own texts in [`crate::messages`]; these are the
//! words of the window itself. Every sentence has a text in each language, and a test checks it.

/// The languages of the window, in the order the lists of words below follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    /// English.
    En,
    /// Spanish.
    Es,
    /// German.
    De,
    /// French.
    Fr,
    /// Italian.
    It,
    /// Japanese.
    Ja,
    /// Chinese.
    Zh,
    /// Portuguese of Portugal.
    Pt,
}

impl Lang {
    /// Every language, in the order of the lists of words.
    pub const ALL: [Lang; 8] = [
        Lang::En,
        Lang::Es,
        Lang::De,
        Lang::Fr,
        Lang::It,
        Lang::Ja,
        Lang::Zh,
        Lang::Pt,
    ];

    /// The two letters of the language (`en`, `es`, `de`, `fr`, `it`, `ja`, `zh`, `pt`).
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Es => "es",
            Lang::De => "de",
            Lang::Fr => "fr",
            Lang::It => "it",
            Lang::Ja => "ja",
            Lang::Zh => "zh",
            Lang::Pt => "pt",
        }
    }

    /// The name of the language in the language itself.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Es => "Español",
            Lang::De => "Deutsch",
            Lang::Fr => "Français",
            Lang::It => "Italiano",
            Lang::Ja => "日本語",
            Lang::Zh => "中文",
            Lang::Pt => "Português",
        }
    }

    /// The language of a code such as `es`, `es_ES.UTF-8` or `pt-PT`.
    pub fn from_code(code: &str) -> Option<Lang> {
        let two: String = code
            .chars()
            .take(2)
            .collect::<String>()
            .to_ascii_lowercase();
        Lang::ALL.into_iter().find(|l| l.code() == two)
    }

    /// The language of the person from the variables of the system; English when it is none of
    /// the eight.
    pub fn from_env() -> Lang {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(var) {
                if !v.is_empty() {
                    return Lang::from_code(&v).unwrap_or(Lang::En);
                }
            }
        }
        Lang::En
    }
}

/// Puts the values in the places `{0}`, `{1}`... of a text.
pub fn fill(template: &str, values: &[&str]) -> String {
    let mut out = template.to_string();
    for (k, v) in values.iter().enumerate() {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// Declares every sentence once, with its eight texts in the order of [`Lang::ALL`].
macro_rules! messages {
    ($($name:ident => [$en:expr, $es:expr, $de:expr, $fr:expr, $it:expr, $ja:expr, $zh:expr, $pt:expr],)*) => {
        /// Every word and sentence of the window.
        #[allow(missing_docs)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Msg {
            $($name,)*
        }

        impl Msg {
            /// Every sentence, once.
            pub fn all() -> &'static [Msg] {
                &[$(Msg::$name,)*]
            }

            /// The words in the order English, Spanish, German, French, Italian, Japanese,
            /// Chinese, Portuguese.
            fn words(self) -> [&'static str; 8] {
                match self {
                    $(Msg::$name => [$en, $es, $de, $fr, $it, $ja, $zh, $pt],)*
                }
            }
        }
    };
}

messages! {
    New => ["New", "Nuevo", "Neu", "Nouveau", "Nuovo", "新規", "新建", "Novo"],
    Open => ["Open...", "Abrir...", "Öffnen...", "Ouvrir...", "Apri...", "開く...", "打开...", "Abrir..."],
    Save => ["Save", "Guardar", "Speichern", "Enregistrer", "Salva", "保存", "保存", "Guardar"],
    SaveAs => ["Save as...", "Guardar como...", "Speichern unter...", "Enregistrer sous...", "Salva con nome...", "名前を付けて保存...", "另存为...", "Guardar como..."],
    Undo => ["Undo", "Deshacer", "Rückgängig", "Annuler", "Annulla", "元に戻す", "撤销", "Desfazer"],
    Redo => ["Redo", "Rehacer", "Wiederholen", "Rétablir", "Ripeti", "やり直す", "重做", "Refazer"],
    Language => ["Language", "Idioma", "Sprache", "Langue", "Lingua", "言語", "语言", "Idioma"],
    FitView => ["Fit view", "Ajustar vista", "Ansicht anpassen", "Ajuster la vue", "Adatta la vista", "表示を合わせる", "适应视图", "Ajustar vista"],
    ModelTitle => ["Model", "Modelo", "Modell", "Modèle", "Modello", "モデル", "模型", "Modelo"],
    PropertiesTitle => ["Properties", "Propiedades", "Eigenschaften", "Propriétés", "Proprietà", "プロパティ", "属性", "Propriedades"],
    View3d => ["3D view", "Vista 3D", "3D-Ansicht", "Vue 3D", "Vista 3D", "3Dビュー", "3D视图", "Vista 3D"],
    ViewPlan => ["Plan view", "Vista de planta", "Draufsicht", "Vue de dessus", "Vista dall'alto", "平面ビュー", "俯视图", "Vista de planta"],
    GroupBox => ["Box", "Caja", "Gehäuse", "Boîtier", "Scatola", "ケース", "外壳", "Caixa"],
    GroupHoles => ["Holes", "Agujeros", "Löcher", "Trous", "Fori", "穴", "孔", "Furos"],
    GroupPosts => ["Posts", "Postes", "Säulen", "Plots", "Colonnine", "ポスト", "立柱", "Postes"],
    GroupEdges => ["Edges", "Cantos", "Kanten", "Arêtes", "Spigoli", "エッジ", "边缘", "Arestas"],
    GroupLid => ["Lid", "Tapa", "Deckel", "Couvercle", "Coperchio", "蓋", "盖子", "Tampa"],
    GroupScrews => ["Lid screws", "Tornillos de la tapa", "Deckelschrauben", "Vis du couvercle", "Viti del coperchio", "蓋のネジ", "盖子螺钉", "Parafusos da tampa"],
    GroupFit => ["Fit check", "Comprobación de ajuste", "Passungsprüfung", "Contrôle d'ajustement", "Verifica di adattamento", "適合チェック", "适配检查", "Verificação de ajuste"],
    GroupFeatures => ["Extrusions and pockets", "Extrusiones y vaciados", "Extrusionen und Taschen", "Extrusions et poches", "Estrusioni e tasche", "押し出しとポケット", "拉伸与切除", "Extrusões e cavidades"],
    AddExtrude => ["Add extrusion", "Añadir extrusión", "Extrusion hinzufügen", "Ajouter une extrusion", "Aggiungi estrusione", "押し出しを追加", "添加拉伸", "Adicionar extrusão"],
    AddPocket => ["Add pocket", "Añadir vaciado", "Tasche hinzufügen", "Ajouter une poche", "Aggiungi tasca", "ポケットを追加", "添加切除", "Adicionar cavidade"],
    NameExtrude => ["Extrusion {0}", "Extrusión {0}", "Extrusion {0}", "Extrusion {0}", "Estrusione {0}", "押し出し {0}", "拉伸 {0}", "Extrusão {0}"],
    NamePocket => ["Pocket {0}", "Vaciado {0}", "Tasche {0}", "Poche {0}", "Tasca {0}", "ポケット {0}", "切除 {0}", "Cavidade {0}"],
    FieldSketch => ["Sketch", "Croquis", "Skizze", "Esquisse", "Schizzo", "スケッチ", "草图", "Esboço"],
    FieldHeight => ["Height", "Altura", "Höhe", "Hauteur", "Altezza", "高さ", "高度", "Altura"],
    FieldPocketDepth => ["Depth", "Profundidad", "Tiefe", "Profondeur", "Profondità", "深さ", "深度", "Profundidade"],
    Through => ["through", "pasante", "durchgehend", "traversant", "passante", "貫通", "贯穿", "passante"],
    SketchKept => ["Sketch {0} kept in the project", "Croquis {0} guardado en el proyecto", "Skizze {0} im Projekt gespeichert", "Esquisse {0} conservée dans le projet", "Schizzo {0} conservato nel progetto", "スケッチ {0} をプロジェクトに保存しました", "草图 {0} 已保存到项目", "Esboço {0} guardado no projeto"],
    KeepSketch => ["Keep in project", "Guardar en el proyecto", "Im Projekt behalten", "Garder dans le projet", "Tieni nel progetto", "プロジェクトに保持", "保存到项目", "Guardar no projeto"],
    GroupOther => ["Other steps", "Otros pasos", "Weitere Schritte", "Autres étapes", "Altri passaggi", "その他の手順", "其他步骤", "Outros passos"],
    Dimensions => ["Dimensions", "Dimensiones", "Abmessungen", "Dimensions", "Dimensioni", "寸法", "尺寸", "Dimensões"],
    Envelope => ["Board envelope", "Envolvente de la placa", "Hüllquader der Platine", "Enveloppe de la carte", "Ingombro della scheda", "基板の外形寸法", "电路板外形", "Envolvente da placa"],
    NothingSelected => ["Select an item of the model.", "Seleccione un elemento del modelo.", "Wählen Sie ein Element des Modells.", "Sélectionnez un élément du modèle.", "Seleziona un elemento del modello.", "モデルの項目を選んでください。", "请选择模型中的一项。", "Selecione um elemento do modelo."],
    EditHint => ["Type a value and press Enter. The box also accepts formulas such as 2 * wall + 1 mm.", "Escriba un valor y pulse Intro. La caja también admite fórmulas como 2 * wall + 1 mm.", "Wert eingeben und Enter drücken. Das Gehäuse akzeptiert auch Formeln wie 2 * wall + 1 mm.", "Saisissez une valeur et appuyez sur Entrée. Le boîtier accepte aussi des formules comme 2 * wall + 1 mm.", "Digita un valore e premi Invio. La scatola accetta anche formule come 2 * wall + 1 mm.", "値を入力して Enter キーを押します。ケースでは 2 * wall + 1 mm のような式も使えます。", "输入数值并按回车键。外壳还可以使用 2 * wall + 1 mm 这样的公式。", "Escreva um valor e prima Enter. A caixa também aceita fórmulas como 2 * wall + 1 mm."],
    StateLabel => ["State", "Estado", "Zustand", "État", "Stato", "状態", "状态", "Estado"],
    StateValid => ["valid", "válido", "gültig", "valide", "valido", "有効", "有效", "válido"],
    StateSuppressed => ["suppressed", "suprimido", "unterdrückt", "supprimé", "soppresso", "抑制中", "已抑制", "suprimido"],
    StateFailed => ["failed", "con fallo", "fehlgeschlagen", "en échec", "fallito", "失敗", "失败", "com falha"],
    StateAfterTip => ["after the tip", "tras la punta", "nach der Spitze", "après la pointe", "dopo la punta", "先端より後", "在末端之后", "após a ponta"],
    SuppressStep => ["Suppress this step", "Suprimir este paso", "Diesen Schritt unterdrücken", "Supprimer cette étape", "Sopprimi questo passaggio", "この手順を抑制", "抑制此步骤", "Suprimir este passo"],
    DerivedNote => ["Derived from the posts: move the posts to move it.", "Derivado de los postes: mueva los postes para moverlo.", "Aus den Säulen abgeleitet: Säulen verschieben, um es zu verschieben.", "Dérivé des plots : déplacez les plots pour le déplacer.", "Derivato dalle colonnine: sposta le colonnine per spostarlo.", "ポストから導出されます。動かすにはポストを動かします。", "由立柱派生：移动立柱即可移动它。", "Derivado dos postes: mova os postes para o mover."],
    FromBoard => ["Taken from the board of the project.", "Tomado de la placa del proyecto.", "Aus der Platine des Projekts übernommen.", "Repris de la carte du projet.", "Preso dalla scheda del progetto.", "プロジェクトの基板から取得しています。", "取自项目的电路板。", "Tomado da placa do projeto."],
    FieldInnerLength => ["Inner length", "Largo interior", "Innenlänge", "Longueur intérieure", "Lunghezza interna", "内寸の長さ", "内部长度", "Comprimento interior"],
    FieldInnerWidth => ["Inner width", "Ancho interior", "Innenbreite", "Largeur intérieure", "Larghezza interna", "内寸の幅", "内部宽度", "Largura interior"],
    FieldInnerHeight => ["Inner height", "Alto interior", "Innenhöhe", "Hauteur intérieure", "Altezza interna", "内寸の高さ", "内部高度", "Altura interior"],
    FieldWall => ["Wall", "Pared", "Wand", "Paroi", "Parete", "壁", "壁厚", "Parede"],
    FieldFloor => ["Floor", "Suelo", "Boden", "Fond", "Fondo", "底", "底板", "Fundo"],
    FieldLid => ["Lid thickness", "Grosor de la tapa", "Deckeldicke", "Épaisseur du couvercle", "Spessore del coperchio", "蓋の厚さ", "盖子厚度", "Espessura da tampa"],
    FieldEdgeMargin => ["Margin from the edges", "Margen a los bordes", "Randabstand", "Marge aux bords", "Margine dai bordi", "端からの余白", "与边缘的间距", "Margem às arestas"],
    FieldMinWeb => ["Least web between holes", "Mínimo entre agujeros", "Mindeststeg zwischen Löchern", "Entretoise minimale entre trous", "Ponte minimo tra i fori", "穴の間の最小幅", "孔间最小间距", "Mínimo entre furos"],
    FieldName => ["Name", "Nombre", "Name", "Nom", "Nome", "名前", "名称", "Nome"],
    FieldFace => ["Face", "Cara", "Fläche", "Face", "Faccia", "面", "面", "Face"],
    FieldU => ["Position u", "Posición u", "Position u", "Position u", "Posizione u", "位置 u", "位置 u", "Posição u"],
    FieldV => ["Position v", "Posición v", "Position v", "Position v", "Posizione v", "位置 v", "位置 v", "Posição v"],
    FieldDiameter => ["Diameter", "Diámetro", "Durchmesser", "Diamètre", "Diametro", "直径", "直径", "Diâmetro"],
    FieldPostX => ["Position x", "Posición x", "Position x", "Position x", "Posizione x", "位置 x", "位置 x", "Posição x"],
    FieldPostY => ["Position y", "Posición y", "Position y", "Position y", "Posizione y", "位置 y", "位置 y", "Posição y"],
    FieldPostOuter => ["Post diameter", "Diámetro del poste", "Säulendurchmesser", "Diamètre du plot", "Diametro della colonnina", "ポストの直径", "立柱直径", "Diâmetro do poste"],
    FieldPostHeight => ["Post height", "Altura del poste", "Säulenhöhe", "Hauteur du plot", "Altezza della colonnina", "ポストの高さ", "立柱高度", "Altura do poste"],
    FieldBoreDiameter => ["Bore diameter", "Diámetro del taladro", "Bohrungsdurchmesser", "Diamètre du perçage", "Diametro del foro", "穴の直径", "孔径", "Diâmetro do furo"],
    FieldBoreDepth => ["Bore depth", "Profundidad del taladro", "Bohrungstiefe", "Profondeur du perçage", "Profondità del foro", "穴の深さ", "孔深", "Profundidade do furo"],
    FieldVertical => ["Vertical edges", "Cantos verticales", "Senkrechte Kanten", "Arêtes verticales", "Spigoli verticali", "垂直のエッジ", "竖直边缘", "Arestas verticais"],
    FieldTopEdges => ["Edges of the lid", "Cantos de la tapa", "Kanten des Deckels", "Arêtes du couvercle", "Spigoli del coperchio", "蓋のエッジ", "盖子边缘", "Arestas da tampa"],
    FieldBottomEdges => ["Edges of the floor", "Cantos del suelo", "Kanten des Bodens", "Arêtes du fond", "Spigoli del fondo", "底のエッジ", "底板边缘", "Arestas do fundo"],
    FieldEdgeSize => ["Size", "Tamaño", "Größe", "Taille", "Dimensione", "サイズ", "尺寸", "Tamanho"],
    FieldLidHole => ["Screw hole diameter", "Diámetro del agujero del tornillo", "Durchmesser der Schraubenbohrung", "Diamètre du trou de vis", "Diametro del foro della vite", "ネジ穴の直径", "螺钉孔直径", "Diâmetro do furo do parafuso"],
    FieldClearance => ["Clearance of the lip", "Holgura del reborde", "Spiel des Rands", "Jeu du rebord", "Gioco del bordino", "リップの隙間", "凸缘间隙", "Folga do ressalto"],
    FieldLipThickness => ["Lip thickness", "Grosor del reborde", "Randdicke", "Épaisseur du rebord", "Spessore del bordino", "リップの厚さ", "凸缘厚度", "Espessura do ressalto"],
    FieldLipDepth => ["Lip depth", "Profundidad del reborde", "Randtiefe", "Profondeur du rebord", "Profondità del bordino", "リップの深さ", "凸缘深度", "Profundidade do ressalto"],
    FieldFitLength => ["Length of the board", "Largo de la placa", "Länge der Platine", "Longueur de la carte", "Lunghezza della scheda", "基板の長さ", "电路板长度", "Comprimento da placa"],
    FieldFitWidth => ["Width of the board", "Ancho de la placa", "Breite der Platine", "Largeur de la carte", "Larghezza della scheda", "基板の幅", "电路板宽度", "Largura da placa"],
    FieldFitHeight => ["Height with its tallest part", "Altura con su pieza más alta", "Höhe mit dem höchsten Bauteil", "Hauteur avec la pièce la plus haute", "Altezza con il componente più alto", "最も高い部品を含む高さ", "含最高元件的高度", "Altura com a peça mais alta"],
    EdgeSharp => ["Sharp", "Vivo", "Scharf", "Vif", "Vivo", "シャープ", "直角", "Vivo"],
    EdgeRound => ["Rounded", "Redondeado", "Gerundet", "Arrondi", "Arrotondato", "丸め", "圆角", "Arredondado"],
    EdgeChamfer => ["Chamfered", "Achaflanado", "Gefast", "Chanfreiné", "Smussato", "面取り", "倒角", "Chanfrado"],
    FaceFront => ["Front", "Frente", "Vorderseite", "Avant", "Fronte", "前面", "前面", "Frente"],
    FaceBack => ["Back", "Trasera", "Rückseite", "Arrière", "Retro", "背面", "后面", "Traseira"],
    FaceLeft => ["Left", "Izquierda", "Links", "Gauche", "Sinistra", "左面", "左面", "Esquerda"],
    FaceRight => ["Right", "Derecha", "Rechts", "Droite", "Destra", "右面", "右面", "Direita"],
    FaceTop => ["Top", "Superior", "Oben", "Dessus", "Sopra", "上面", "顶面", "Superior"],
    FaceBottom => ["Bottom", "Inferior", "Unten", "Dessous", "Sotto", "下面", "底面", "Inferior"],
    OpenTitle => ["Open a project", "Abrir un proyecto", "Projekt öffnen", "Ouvrir un projet", "Apri un progetto", "プロジェクトを開く", "打开项目", "Abrir um projeto"],
    SaveTitle => ["Save the project as", "Guardar el proyecto como", "Projekt speichern unter", "Enregistrer le projet sous", "Salva il progetto come", "プロジェクトに名前を付けて保存", "项目另存为", "Guardar o projeto como"],
    Up => ["Up", "Subir", "Hoch", "Monter", "Su", "上へ", "上一级", "Subir"],
    AllFiles => ["All files", "Todos los ficheros", "Alle Dateien", "Tous les fichiers", "Tutti i file", "すべてのファイル", "所有文件", "Todos os ficheiros"],
    Cancel => ["Cancel", "Cancelar", "Abbrechen", "Annuler", "Annulla", "キャンセル", "取消", "Cancelar"],
    EmptyFolder => ["Nothing to show in this folder.", "No hay nada que mostrar en esta carpeta.", "In diesem Ordner gibt es nichts anzuzeigen.", "Rien à afficher dans ce dossier.", "Niente da mostrare in questa cartella.", "このフォルダーには表示するものがありません。", "此文件夹中没有可显示的内容。", "Nada para mostrar nesta pasta."],
    FileName => ["File name", "Nombre del fichero", "Dateiname", "Nom du fichier", "Nome del file", "ファイル名", "文件名", "Nome do ficheiro"],
    Ready => ["Ready", "Listo", "Bereit", "Prêt", "Pronto", "準備完了", "就绪", "Pronto"],
    Opened => ["Opened {0}", "Abierto {0}", "{0} geöffnet", "{0} ouvert", "Aperto {0}", "{0} を開きました", "已打开 {0}", "Aberto {0}"],
    Saved => ["Saved {0}", "Guardado {0}", "{0} gespeichert", "{0} enregistré", "Salvato {0}", "{0} を保存しました", "已保存 {0}", "Guardado {0}"],
    NewProject => ["New project", "Proyecto nuevo", "Neues Projekt", "Nouveau projet", "Nuovo progetto", "新しいプロジェクト", "新项目", "Projeto novo"],
    Undone => ["Undone: {0}", "Deshecho: {0}", "Rückgängig gemacht: {0}", "Annulé : {0}", "Annullato: {0}", "元に戻しました: {0}", "已撤销：{0}", "Desfeito: {0}"],
    Redone => ["Redone: {0}", "Rehecho: {0}", "Wiederholt: {0}", "Rétabli : {0}", "Ripetuto: {0}", "やり直しました: {0}", "已重做：{0}", "Refeito: {0}"],
    NothingToUndo => ["Nothing to undo", "Nada que deshacer", "Nichts rückgängig zu machen", "Rien à annuler", "Niente da annullare", "元に戻す操作はありません", "没有可撤销的操作", "Nada para desfazer"],
    NothingToRedo => ["Nothing to redo", "Nada que rehacer", "Nichts zu wiederholen", "Rien à rétablir", "Niente da ripetere", "やり直す操作はありません", "没有可重做的操作", "Nada para refazer"],
    ChangeOf => ["Change of {0}", "Cambio de {0}", "Änderung von {0}", "Modification de {0}", "Modifica di {0}", "{0} の変更", "修改 {0}", "Alteração de {0}"],
    ToggleStep => ["Suppress or restore {0}", "Suprimir o restaurar {0}", "{0} unterdrücken oder wiederherstellen", "Supprimer ou rétablir {0}", "Sopprimere o ripristinare {0}", "{0} を抑制または復元", "抑制或恢复 {0}", "Suprimir ou restaurar {0}"],
    Refused => ["Not changed: {0}", "Sin cambios: {0}", "Nicht geändert: {0}", "Pas de changement : {0}", "Nessuna modifica: {0}", "変更されませんでした: {0}", "未更改：{0}", "Sem alterações: {0}"],
    ReadOnlyOpened => ["This project has parts this program does not understand: it is open read-only.", "Este proyecto tiene partes que el programa no entiende: se abre de solo lectura.", "Dieses Projekt enthält Teile, die das Programm nicht versteht: Es wird schreibgeschützt geöffnet.", "Ce projet contient des parties que le programme ne comprend pas : il est ouvert en lecture seule.", "Questo progetto ha parti che il programma non capisce: è aperto in sola lettura.", "このプロジェクトにはプログラムが理解できない部分があるため、読み取り専用で開きました。", "此项目包含程序无法理解的部分，因此以只读方式打开。", "Este projeto tem partes que o programa não entende: abre só de leitura."],
    ReadOnlyBadge => ["Read-only", "Solo lectura", "Schreibgeschützt", "Lecture seule", "Sola lettura", "読み取り専用", "只读", "Só de leitura"],
    NoMechanical => ["The project has no mechanical part.", "El proyecto no tiene parte mecánica.", "Das Projekt hat keinen mechanischen Teil.", "Le projet n'a pas de partie mécanique.", "Il progetto non ha una parte meccanica.", "プロジェクトに機構部分がありません。", "该项目没有机械部分。", "O projeto não tem parte mecânica."],
    Measures => ["Outside {0} x {1} x {2} mm - inside {3} x {4} x {5} mm - material {6} cm3", "Exterior {0} x {1} x {2} mm - interior {3} x {4} x {5} mm - material {6} cm3", "Außen {0} x {1} x {2} mm - innen {3} x {4} x {5} mm - Material {6} cm3", "Extérieur {0} x {1} x {2} mm - intérieur {3} x {4} x {5} mm - matière {6} cm3", "Esterno {0} x {1} x {2} mm - interno {3} x {4} x {5} mm - materiale {6} cm3", "外寸 {0} x {1} x {2} mm - 内寸 {3} x {4} x {5} mm - 材料 {6} cm3", "外部 {0} x {1} x {2} mm - 内部 {3} x {4} x {5} mm - 材料 {6} cm3", "Exterior {0} x {1} x {2} mm - interior {3} x {4} x {5} mm - material {6} cm3"],
    FitOk => ["It fits, with {0} / {1} / {2} mm to spare", "Cabe, con {0} / {1} / {2} mm de sobra", "Passt, mit {0} / {1} / {2} mm Spiel", "Ça rentre, avec {0} / {1} / {2} mm de marge", "Ci sta, con {0} / {1} / {2} mm in più", "収まります（余裕 {0} / {1} / {2} mm）", "可以装入，余量 {0} / {1} / {2} mm", "Cabe, com {0} / {1} / {2} mm de sobra"],
    FitTurned => ["It does not fit as placed; it fits turned a quarter of a turn", "No cabe como está; cabe girada un cuarto de vuelta", "Passt so nicht; es passt um eine Vierteldrehung gedreht", "Ne rentre pas ainsi ; rentre tournée d'un quart de tour", "Non ci sta così; ci sta ruotata di un quarto di giro", "このままでは収まりません。90度回すと収まります", "按当前放置无法装入；旋转四分之一圈后可以装入", "Não cabe assim; cabe rodada um quarto de volta"],
    FitNo => ["It does not fit ({0} / {1} / {2} mm)", "No cabe ({0} / {1} / {2} mm)", "Passt nicht ({0} / {1} / {2} mm)", "Ne rentre pas ({0} / {1} / {2} mm)", "Non ci sta ({0} / {1} / {2} mm)", "収まりません（{0} / {1} / {2} mm）", "无法装入（{0} / {1} / {2} mm）", "Não cabe ({0} / {1} / {2} mm)"],
    StepsFailed => ["{0} step(s) failed", "{0} paso(s) con fallo", "{0} Schritt(e) fehlgeschlagen", "{0} étape(s) en échec", "{0} passaggio/i fallito/i", "{0} 件の手順が失敗", "{0} 个步骤失败", "{0} passo(s) com falha"],
    Unsaved => ["unsaved changes", "cambios sin guardar", "ungespeicherte Änderungen", "modifications non enregistrées", "modifiche non salvate", "未保存の変更", "有未保存的更改", "alterações por guardar"],
    ViewHint3d => ["Drag to turn, wheel to zoom, double-click to reset", "Arrastre para girar, rueda para ampliar, doble clic para restablecer", "Ziehen zum Drehen, Rad zum Zoomen, Doppelklick zum Zurücksetzen", "Glissez pour tourner, molette pour zoomer, double-clic pour réinitialiser", "Trascina per ruotare, rotellina per ingrandire, doppio clic per ripristinare", "ドラッグで回転、ホイールで拡大縮小、ダブルクリックでリセット", "拖动旋转，滚轮缩放，双击复位", "Arraste para rodar, roda para ampliar, duplo clique para repor"],
    ViewHintPlan => ["Drag to move, wheel to zoom, double-click to reset", "Arrastre para mover, rueda para ampliar, doble clic para restablecer", "Ziehen zum Verschieben, Rad zum Zoomen, Doppelklick zum Zurücksetzen", "Glissez pour déplacer, molette pour zoomer, double-clic pour réinitialiser", "Trascina per spostare, rotellina per ingrandire, doppio clic per ripristinare", "ドラッグで移動、ホイールで拡大縮小、ダブルクリックでリセット", "拖动平移，滚轮缩放，双击复位", "Arraste para mover, roda para ampliar, duplo clique para repor"],
    LegendBoard => ["Board", "Placa", "Platine", "Carte", "Scheda", "基板", "电路板", "Placa"],
    GroupVariables => ["Variables", "Variables", "Variablen", "Variables", "Variabili", "変数", "变量", "Variáveis"],
    SheetTitle => ["Sheet of variables", "Hoja de variables", "Variablentabelle", "Feuille de variables", "Foglio delle variabili", "変数シート", "变量表", "Folha de variáveis"],
    SheetHint => [
        "Type a length (30 mm) or a formula (length / 2 + 5 mm). Every parameter that uses the variable is calculated again.",
        "Escriba una longitud (30 mm) o una fórmula (largo / 2 + 5 mm). Todos los parámetros que usan la variable se calculan de nuevo.",
        "Geben Sie eine Länge (30 mm) oder eine Formel (laenge / 2 + 5 mm) ein. Jeder Parameter, der die Variable verwendet, wird neu berechnet.",
        "Saisissez une longueur (30 mm) ou une formule (longueur / 2 + 5 mm). Tous les paramètres qui utilisent la variable sont recalculés.",
        "Scriva una lunghezza (30 mm) o una formula (lunghezza / 2 + 5 mm). Ogni parametro che usa la variabile viene ricalcolato.",
        "長さ (30 mm) または式 (length / 2 + 5 mm) を入力してください。その変数を使うすべてのパラメータが再計算されます。",
        "输入长度 (30 mm) 或公式 (length / 2 + 5 mm)。使用该变量的所有参数都会重新计算。",
        "Escreva um comprimento (30 mm) ou uma fórmula (comprimento / 2 + 5 mm). Todos os parâmetros que usam a variável são recalculados."
    ],
    VariableName => ["Name", "Nombre", "Name", "Nom", "Nome", "名前", "名称", "Nome"],
    VariableValue => ["Value or formula", "Valor o fórmula", "Wert oder Formel", "Valeur ou formule", "Valore o formula", "値または式", "值或公式", "Valor ou fórmula"],
    VariableResult => ["Calculated", "Calculado", "Berechnet", "Calculé", "Calcolato", "計算値", "计算值", "Calculado"],
    VariableUsedBy => ["Used by", "Usada por", "Verwendet von", "Utilisée par", "Usata da", "使用先", "使用者", "Usada por"],
    VariableUnused => ["not used", "sin usar", "nicht verwendet", "inutilisée", "non usata", "未使用", "未使用", "sem uso"],
    AddVariable => ["Add variable", "Añadir variable", "Variable hinzufügen", "Ajouter une variable", "Aggiungi variabile", "変数を追加", "添加变量", "Adicionar variável"],
    RemoveVariable => ["Remove", "Quitar", "Entfernen", "Retirer", "Rimuovi", "削除", "删除", "Remover"],
    VariableChanged => ["Variable {0} = {1}", "Variable {0} = {1}", "Variable {0} = {1}", "Variable {0} = {1}", "Variabile {0} = {1}", "変数 {0} = {1}", "变量 {0} = {1}", "Variável {0} = {1}"],
    VariableRemoved => ["Variable {0} removed", "Variable {0} quitada", "Variable {0} entfernt", "Variable {0} retirée", "Variabile {0} rimossa", "変数 {0} を削除しました", "已删除变量 {0}", "Variável {0} removida"],
    AddHole => ["Add hole", "Añadir agujero", "Loch hinzufügen", "Ajouter un trou", "Aggiungi foro", "穴を追加", "添加孔", "Adicionar furo"],
    AddPost => ["Add post", "Añadir poste", "Säule hinzufügen", "Ajouter un plot", "Aggiungi colonnina", "ポストを追加", "添加立柱", "Adicionar poste"],
    AddEdging => ["Add edge treatment", "Añadir tratamiento de cantos", "Kantenbearbeitung hinzufügen", "Ajouter un traitement des arêtes", "Aggiungi trattamento spigoli", "エッジ処理を追加", "添加边缘处理", "Adicionar tratamento de arestas"],
    RemoveStep => ["Remove step", "Quitar paso", "Schritt entfernen", "Retirer l'étape", "Rimuovi passaggio", "ステップを削除", "删除步骤", "Remover passo"],
    NameHole => ["Hole {0}", "Agujero {0}", "Loch {0}", "Trou {0}", "Foro {0}", "穴 {0}", "孔 {0}", "Furo {0}"],
    NamePost => ["Post {0}", "Poste {0}", "Säule {0}", "Plot {0}", "Colonnina {0}", "ポスト {0}", "立柱 {0}", "Poste {0}"],
    NameEdging => ["Edge treatment", "Tratamiento de cantos", "Kantenbearbeitung", "Traitement des arêtes", "Trattamento spigoli", "エッジ処理", "边缘处理", "Tratamento de arestas"],
    StepAdded => ["Added {0}", "Añadido {0}", "{0} hinzugefügt", "{0} ajouté", "Aggiunto {0}", "{0} を追加しました", "已添加 {0}", "Adicionado {0}"],
    StepRemoved => ["Removed {0}", "Quitado {0}", "{0} entfernt", "{0} retiré", "Rimosso {0}", "{0} を削除しました", "已删除 {0}", "Removido {0}"],
    WorkspaceSketch => ["Sketch", "Croquis", "Skizze", "Esquisse", "Schizzo", "スケッチ", "草图", "Esboço"],
    ToolsTitle => ["Tools", "Herramientas", "Werkzeuge", "Outils", "Strumenti", "ツール", "工具", "Ferramentas"],
    ToolSelect => ["Select", "Seleccionar", "Auswählen", "Sélectionner", "Seleziona", "選択", "选择", "Selecionar"],
    ToolPoint => ["Point", "Punto", "Punkt", "Point", "Punto", "点", "点", "Ponto"],
    ToolLine => ["Line", "Línea", "Linie", "Ligne", "Linea", "線", "直线", "Linha"],
    ToolPolyline => ["Polyline", "Polilínea", "Polylinie", "Polyligne", "Polilinea", "ポリライン", "折线", "Polilinha"],
    ToolRectangle => ["Rectangle", "Rectángulo", "Rechteck", "Rectangle", "Rettangolo", "長方形", "矩形", "Retângulo"],
    ToolCircle => ["Circle", "Círculo", "Kreis", "Cercle", "Cerchio", "円", "圆", "Círculo"],
    ToolArc => ["Arc", "Arco", "Bogen", "Arc", "Arco", "円弧", "圆弧", "Arco"],
    HintSelect => ["Click an element to pick it, drag to move it, hold Ctrl to pick more than one.", "Haga clic en un elemento para elegirlo, arrastre para moverlo, con Ctrl elige varios.", "Element anklicken zum Auswählen, ziehen zum Verschieben, mit Strg mehrere wählen.", "Cliquez sur un élément pour le choisir, faites-le glisser pour le déplacer, Ctrl pour en choisir plusieurs.", "Fai clic su un elemento per sceglierlo, trascinalo per spostarlo, con Ctrl ne scegli più di uno.", "要素をクリックして選択、ドラッグして移動、Ctrl で複数選択します。", "单击元素选中，拖动移动，按住 Ctrl 可多选。", "Clique num elemento para o escolher, arraste para o mover, com Ctrl escolhe vários."],
    HintPoint => ["Click where the point goes.", "Haga clic donde va el punto.", "Klicken, wo der Punkt hin soll.", "Cliquez à l'endroit du point.", "Fai clic dove va il punto.", "点を置く位置をクリックします。", "单击点的位置。", "Clique onde fica o ponto."],
    HintLine => ["Click the start and then the end of the line.", "Haga clic en el inicio y luego en el final de la línea.", "Anfang und dann Ende der Linie anklicken.", "Cliquez sur le début puis sur la fin de la ligne.", "Fai clic sull'inizio e poi sulla fine della linea.", "線の始点、次に終点をクリックします。", "依次单击直线的起点和终点。", "Clique no início e depois no fim da linha."],
    HintPolyline => ["Click each corner; Enter, a right click or a click on the first corner finishes.", "Haga clic en cada vértice; Intro, clic derecho o clic en el primer vértice termina.", "Jede Ecke anklicken; Enter, Rechtsklick oder Klick auf die erste Ecke beendet.", "Cliquez sur chaque sommet ; Entrée, clic droit ou clic sur le premier sommet termine.", "Fai clic su ogni vertice; Invio, clic destro o clic sul primo vertice termina.", "各頂点をクリックし、Enter・右クリック・最初の頂点のクリックで終了します。", "依次单击各顶点；按 Enter、右键单击或单击第一个顶点结束。", "Clique em cada vértice; Enter, clique direito ou clique no primeiro vértice termina."],
    HintRectangle => ["Click two opposite corners.", "Haga clic en dos esquinas opuestas.", "Zwei gegenüberliegende Ecken anklicken.", "Cliquez sur deux coins opposés.", "Fai clic su due angoli opposti.", "対角の2点をクリックします。", "单击两个对角。", "Clique em dois cantos opostos."],
    HintCircle => ["Click the centre and then a point of the circle.", "Haga clic en el centro y luego en un punto del círculo.", "Mittelpunkt und dann einen Punkt des Kreises anklicken.", "Cliquez sur le centre puis sur un point du cercle.", "Fai clic sul centro e poi su un punto del cerchio.", "中心、次に円周上の点をクリックします。", "依次单击圆心和圆上一点。", "Clique no centro e depois num ponto do círculo."],
    HintArc => ["Click the centre, the start and the end (the arc runs counter-clockwise).", "Haga clic en el centro, el inicio y el final (el arco va en sentido antihorario).", "Mittelpunkt, Anfang und Ende anklicken (der Bogen läuft gegen den Uhrzeigersinn).", "Cliquez sur le centre, le début et la fin (l'arc va dans le sens antihoraire).", "Fai clic su centro, inizio e fine (l'arco va in senso antiorario).", "中心、始点、終点の順にクリックします（円弧は反時計回り）。", "依次单击圆心、起点和终点（圆弧按逆时针方向）。", "Clique no centro, no início e no fim (o arco segue o sentido anti-horário)."],
    HintHorizontal => ["It will be made horizontal.", "Se hará horizontal.", "Wird waagerecht gemacht.", "Elle sera rendue horizontale.", "Sarà resa orizzontale.", "水平にします。", "将设为水平。", "Ficará horizontal."],
    HintVertical => ["It will be made vertical.", "Se hará vertical.", "Wird senkrecht gemacht.", "Elle sera rendue verticale.", "Sarà resa verticale.", "垂直にします。", "将设为垂直。", "Ficará vertical."],
    CoCoincident => ["Coincident", "Coincidente", "Deckungsgleich", "Coïncident", "Coincidente", "一致", "重合", "Coincidente"],
    CoPointOnObject => ["Point on object", "Punto sobre objeto", "Punkt auf Objekt", "Point sur objet", "Punto su oggetto", "点を対象上に", "点在对象上", "Ponto sobre objeto"],
    CoHorizontal => ["Horizontal", "Horizontal", "Horizontal", "Horizontal", "Orizzontale", "水平", "水平", "Horizontal"],
    CoVertical => ["Vertical", "Vertical", "Vertikal", "Vertical", "Verticale", "垂直", "垂直", "Vertical"],
    CoParallel => ["Parallel", "Paralela", "Parallel", "Parallèle", "Parallela", "平行", "平行", "Paralela"],
    CoPerpendicular => ["Perpendicular", "Perpendicular", "Senkrecht", "Perpendiculaire", "Perpendicolare", "直交", "垂直相交", "Perpendicular"],
    CoTangent => ["Tangent", "Tangente", "Tangential", "Tangente", "Tangente", "接線", "相切", "Tangente"],
    CoEqual => ["Equal", "Igual", "Gleich", "Égal", "Uguale", "等しい", "相等", "Igual"],
    CoSymmetric => ["Symmetric", "Simétrica", "Symmetrisch", "Symétrique", "Simmetrica", "対称", "对称", "Simétrica"],
    CoMidpoint => ["Midpoint", "Punto medio", "Mittelpunkt", "Milieu", "Punto medio", "中点", "中点", "Ponto médio"],
    CoFixed => ["Fixed", "Fija", "Fixiert", "Fixe", "Fissa", "固定", "固定", "Fixa"],
    DiDistance => ["Distance", "Distancia", "Abstand", "Distance", "Distanza", "距離", "距离", "Distância"],
    DiDistanceX => ["Horizontal distance", "Distancia horizontal", "Horizontaler Abstand", "Distance horizontale", "Distanza orizzontale", "水平距離", "水平距离", "Distância horizontal"],
    DiDistanceY => ["Vertical distance", "Distancia vertical", "Vertikaler Abstand", "Distance verticale", "Distanza verticale", "垂直距離", "垂直距离", "Distância vertical"],
    DiLength => ["Length", "Longitud", "Länge", "Longueur", "Lunghezza", "長さ", "长度", "Comprimento"],
    DiRadius => ["Radius", "Radio", "Radius", "Rayon", "Raggio", "半径", "半径", "Raio"],
    DiDiameter => ["Diameter", "Diámetro", "Durchmesser", "Diamètre", "Diametro", "直径", "直径", "Diâmetro"],
    DiAngle => ["Angle", "Ángulo", "Winkel", "Angle", "Angolo", "角度", "角度", "Ângulo"],
    ElPoint => ["Point", "Punto", "Punkt", "Point", "Punto", "点", "点", "Ponto"],
    ElLine => ["Line", "Línea", "Linie", "Ligne", "Linea", "線", "直线", "Linha"],
    ElCircle => ["Circle", "Círculo", "Kreis", "Cercle", "Cerchio", "円", "圆", "Círculo"],
    ElArc => ["Arc", "Arco", "Bogen", "Arc", "Arco", "円弧", "圆弧", "Arco"],
    SketchNew => ["New sketch", "Croquis nuevo", "Neue Skizze", "Nouvelle esquisse", "Nuovo schizzo", "新しいスケッチ", "新草图", "Esboço novo"],
    ElementDrawn => ["Drew: {0}", "Dibujado: {0}", "Gezeichnet: {0}", "Dessiné : {0}", "Disegnato: {0}", "描画しました: {0}", "已绘制：{0}", "Desenhado: {0}"],
    TooShort => ["Too short: pick another place.", "Demasiado corto: elija otro lugar.", "Zu kurz: einen anderen Ort wählen.", "Trop court : choisissez un autre endroit.", "Troppo corto: scegli un altro punto.", "短すぎます。別の位置を選んでください。", "太短：请选择其他位置。", "Demasiado curto: escolha outro sítio."],
    AlreadyAPoint => ["There is already a point there.", "Ya hay un punto ahí.", "Dort liegt schon ein Punkt.", "Il y a déjà un point ici.", "Qui c'è già un punto.", "そこには既に点があります。", "此处已有一个点。", "Já existe um ponto aí."],
    ConstraintAdded => ["Added: {0}", "Añadido: {0}", "Hinzugefügt: {0}", "Ajouté : {0}", "Aggiunto: {0}", "追加しました: {0}", "已添加：{0}", "Adicionado: {0}"],
    ValueNeeded => ["Type a value, a variable or a formula.", "Escriba un valor, una variable o una fórmula.", "Wert, Variable oder Formel eingeben.", "Saisissez une valeur, une variable ou une formule.", "Scrivi un valore, una variabile o una formula.", "値、変数、または式を入力してください。", "请输入数值、变量或公式。", "Escreva um valor, uma variável ou uma fórmula."],
    DimensionChanged => ["{0} = {1}", "{0} = {1}", "{0} = {1}", "{0} = {1}", "{0} = {1}", "{0} = {1}", "{0} = {1}", "{0} = {1}"],
    StateEmpty => ["Empty sketch", "Croquis vacío", "Leere Skizze", "Esquisse vide", "Schizzo vuoto", "空のスケッチ", "空草图", "Esboço vazio"],
    StateFull => ["Fully constrained", "Totalmente restringido", "Vollständig bestimmt", "Entièrement contraint", "Completamente vincolato", "完全拘束", "完全约束", "Totalmente restringido"],
    StateUnder => ["{0} degrees of freedom left", "Quedan {0} grados de libertad", "{0} Freiheitsgrade übrig", "Il reste {0} degrés de liberté", "Restano {0} gradi di libertà", "自由度が {0} 残っています", "还剩 {0} 个自由度", "{0} graus de liberdade por fixar"],
    StateOver => ["Over-constrained", "Sobrerrestringido", "Überbestimmt", "Surcontraint", "Sovravincolato", "拘束過多", "过约束", "Sobrerrestringido"],
    StateNotSolved => ["No solution found", "Sin solución", "Keine Lösung gefunden", "Aucune solution trouvée", "Nessuna soluzione trovata", "解が見つかりません", "未找到解", "Sem solução"],
    ConflictList => ["In conflict: {0}", "En conflicto: {0}", "Im Widerspruch: {0}", "En conflit : {0}", "In conflitto: {0}", "競合: {0}", "冲突：{0}", "Em conflito: {0}"],
    RedundantList => ["Redundant: {0}", "Redundantes: {0}", "Überflüssig: {0}", "Redondantes : {0}", "Ridondanti: {0}", "冗長: {0}", "冗余：{0}", "Redundantes: {0}"],
    DisableProblems => ["Disable these constraints", "Desactivar estas restricciones", "Diese Bedingungen ausschalten", "Désactiver ces contraintes", "Disattiva questi vincoli", "これらの拘束を無効にする", "停用这些约束", "Desativar estas restrições"],
    ProblemsDisabled => ["Disabled {0} constraint(s)", "Desactivadas {0} restricción(es)", "{0} Bedingung(en) ausgeschaltet", "{0} contrainte(s) désactivée(s)", "Disattivati {0} vincolo/i", "{0} 件の拘束を無効にしました", "已停用 {0} 个约束", "Desativadas {0} restrição(ões)"],
    SelectFirst => ["Select something first.", "Seleccione algo primero.", "Zuerst etwas auswählen.", "Sélectionnez d'abord quelque chose.", "Seleziona prima qualcosa.", "先に対象を選択してください。", "请先选择对象。", "Selecione primeiro alguma coisa."],
    ElementsDeleted => ["Deleted {0} item(s)", "Eliminados {0} elemento(s)", "{0} Element(e) gelöscht", "{0} élément(s) supprimé(s)", "Eliminati {0} elemento/i", "{0} 件を削除しました", "已删除 {0} 项", "Eliminados {0} elemento(s)"],
    DeleteSelection => ["Delete", "Eliminar", "Löschen", "Supprimer", "Elimina", "削除", "删除", "Eliminar"],
    ToggleConstruction => ["Toggle construction", "Alternar construcción", "Konstruktion umschalten", "Basculer en construction", "Commuta costruzione", "補助線を切り替え", "切换构造线", "Alternar construção"],
    HistMove => ["Move", "Mover", "Verschieben", "Déplacer", "Sposta", "移動", "移动", "Mover"],
    ElementMoved => ["Moved", "Movido", "Verschoben", "Déplacé", "Spostato", "移動しました", "已移动", "Movido"],
    ConstructionMode => ["Draw as construction", "Dibujar como construcción", "Als Konstruktion zeichnen", "Dessiner en construction", "Disegna come costruzione", "補助線として描く", "作为构造线绘制", "Desenhar como construção"],
    GridSnap => ["Snap to grid", "Ajustar a la rejilla", "Am Raster einrasten", "Aimanter à la grille", "Aggancia alla griglia", "グリッドにスナップ", "吸附到网格", "Ajustar à grelha"],
    AutoConstraints => ["Propose constraints", "Proponer restricciones", "Bedingungen vorschlagen", "Proposer des contraintes", "Proponi vincoli", "拘束を提案", "建议约束", "Propor restrições"],
    ConstraintsTitle => ["Constraints", "Restricciones", "Bedingungen", "Contraintes", "Vincoli", "拘束", "约束", "Restrições"],
    DimensionsTitle => ["Dimensions", "Cotas", "Maße", "Cotes", "Quote", "寸法", "尺寸标注", "Cotas"],
    ElementsTitle => ["Elements", "Elementos", "Elemente", "Éléments", "Elementi", "要素", "元素", "Elementos"],
    AddConstraintHint => ["Select elements on the canvas, then choose a constraint. A right click on the canvas offers the same.", "Seleccione elementos en el lienzo y elija una restricción. El clic derecho sobre el lienzo ofrece lo mismo.", "Elemente auf der Zeichenfläche wählen, dann eine Bedingung. Rechtsklick auf die Zeichenfläche bietet dasselbe.", "Sélectionnez des éléments sur la zone de dessin puis une contrainte. Le clic droit sur la zone offre la même chose.", "Seleziona elementi sull'area di disegno e scegli un vincolo. Il clic destro sull'area offre lo stesso.", "キャンバス上で要素を選び、拘束を選びます。キャンバスを右クリックしても同じ操作ができます。", "在画布上选择元素，然后选择约束。在画布上单击右键也可以。", "Selecione elementos na área de desenho e escolha uma restrição. O clique direito na área de desenho oferece o mesmo."],
    DimensionsHint => ["Select what to measure, type a value or a variable (optional) and choose the dimension.", "Seleccione lo que acotar, escriba un valor o una variable (opcional) y elija la cota.", "Wählen Sie, was bemaßt wird, geben Sie optional einen Wert oder eine Variable ein und wählen Sie das Maß.", "Sélectionnez ce qu'il faut coter, saisissez une valeur ou une variable (facultatif) et choisissez la cote.", "Seleziona cosa quotare, scrivi un valore o una variabile (facoltativo) e scegli la quota.", "寸法を付ける対象を選び、値または変数を入力（省略可）して、寸法の種類を選びます。", "选择要标注的对象，输入数值或变量（可选），然后选择尺寸类型。", "Selecione o que cotar, escreva um valor ou uma variável (opcional) e escolha a cota."],
    DimValueHint => ["Value or variable: 40, 1.5 in, width / 2 + 2 mm", "Valor o variable: 40, 1.5 in, width / 2 + 2 mm", "Wert oder Variable: 40, 1.5 in, width / 2 + 2 mm", "Valeur ou variable : 40, 1.5 in, width / 2 + 2 mm", "Valore o variabile: 40, 1.5 in, width / 2 + 2 mm", "値または変数: 40, 1.5 in, width / 2 + 2 mm", "数值或变量：40, 1.5 in, width / 2 + 2 mm", "Valor ou variável: 40, 1.5 in, width / 2 + 2 mm"],
    ConstraintEnabled => ["Enabled", "Activa", "Aktiv", "Active", "Attivo", "有効", "启用", "Ativa"],
    NoConstraints => ["No constraints yet.", "Aún no hay restricciones.", "Noch keine Bedingungen.", "Pas encore de contraintes.", "Ancora nessun vincolo.", "まだ拘束はありません。", "尚无约束。", "Ainda não há restrições."],
    OpenSketchTitle => ["Open a sketch", "Abrir un croquis", "Skizze öffnen", "Ouvrir une esquisse", "Apri uno schizzo", "スケッチを開く", "打开草图", "Abrir um esboço"],
    SaveSketchTitle => ["Save the sketch as", "Guardar el croquis como", "Skizze speichern unter", "Enregistrer l'esquisse sous", "Salva lo schizzo come", "スケッチに名前を付けて保存", "草图另存为", "Guardar o esboço como"],
    CanvasHint => ["Wheel to zoom, middle button or Space and drag to pan, double-click to fit", "Rueda para ampliar, botón central o Espacio y arrastrar para desplazar, doble clic para ajustar", "Rad zum Zoomen, mittlere Taste oder Leertaste und Ziehen zum Verschieben, Doppelklick zum Anpassen", "Molette pour zoomer, bouton du milieu ou Espace et glisser pour déplacer, double-clic pour ajuster", "Rotella per zoomare, tasto centrale o Spazio e trascina per spostare, doppio clic per adattare", "ホイールで拡大縮小、中ボタンまたはスペース＋ドラッグで移動、ダブルクリックで全体表示", "滚轮缩放，中键或空格加拖动平移，双击适应", "Roda para ampliar, botão do meio ou Espaço e arrastar para deslocar, duplo clique para ajustar"],
    DragName => ["Move {0}", "Mover {0}", "{0} verschieben", "Déplacer {0}", "Sposta {0}", "{0} を移動", "移动 {0}", "Mover {0}"],
    DragDone => ["Moved {0} to ({1}, {2})", "{0} movido a ({1}, {2})", "{0} nach ({1}, {2}) verschoben", "{0} déplacé en ({1}, {2})", "{0} spostato in ({1}, {2})", "{0} を ({1}, {2}) に移動しました", "{0} 已移动到 ({1}, {2})", "{0} movido para ({1}, {2})"],
    DragCancelled => ["Move cancelled", "Movimiento cancelado", "Verschieben abgebrochen", "Déplacement annulé", "Spostamento annullato", "移動を取り消しました", "已取消移动", "Movimento cancelado"],
    DragHint => ["Drag a hole or a post to move it; Esc cancels", "Arrastre un agujero o un poste para moverlo; Esc cancela", "Loch oder Säule ziehen, um sie zu verschieben; Esc bricht ab", "Glissez un trou ou un plot pour le déplacer ; Échap annule", "Trascina un foro o una colonnina per spostarli; Esc annulla", "穴やポストをドラッグして移動します。Esc で取り消し", "拖动孔或立柱即可移动；按 Esc 取消", "Arraste um furo ou um poste para o mover; Esc cancela"],
    MeasureTitle => ["Measurements", "Mediciones", "Messungen", "Mesures", "Misure", "測定", "测量", "Medições"],
    MeasureDistanceTool => ["Measure distance", "Medir distancia", "Abstand messen", "Mesurer une distance", "Misura distanza", "距離を測定", "测量距离", "Medir distância"],
    MeasureAngleTool => ["Measure angle", "Medir ángulo", "Winkel messen", "Mesurer un angle", "Misura angolo", "角度を測定", "测量角度", "Medir ângulo"],
    MeasureClear => ["Clear readings", "Borrar mediciones", "Messungen löschen", "Effacer les mesures", "Cancella le misure", "測定結果を消去", "清除测量", "Limpar medições"],
    MeasureHint => ["Pick two points or edges for a distance; two edges, or three points with the vertex second, for an angle.", "Elija dos puntos o aristas para una distancia; dos aristas, o tres puntos con el vértice en segundo lugar, para un ángulo.", "Zwei Punkte oder Kanten für einen Abstand wählen; zwei Kanten oder drei Punkte (der zweite ist der Scheitel) für einen Winkel.", "Choisissez deux points ou arêtes pour une distance ; deux arêtes, ou trois points avec le sommet en second, pour un angle.", "Scegli due punti o spigoli per una distanza; due spigoli, o tre punti con il vertice al secondo posto, per un angolo.", "距離は点または辺を 2 つ、角度は辺を 2 つ、または頂点を 2 番目にした点を 3 つ選びます。", "距离：选择两个点或两条边；角度：选择两条边，或三个点（第二个为顶点）。", "Escolha dois pontos ou arestas para uma distância; duas arestas, ou três pontos com o vértice em segundo lugar, para um ângulo."],
    MeasurePickFirst => ["Pick the first point or edge", "Elija el primer punto o arista", "Ersten Punkt oder erste Kante wählen", "Choisissez le premier point ou la première arête", "Scegli il primo punto o spigolo", "1 つ目の点または辺を選んでください", "请选择第一个点或边", "Escolha o primeiro ponto ou aresta"],
    MeasurePickNext => ["Pick the next one ({0} of {1})", "Elija el siguiente ({0} de {1})", "Nächstes wählen ({0} von {1})", "Choisissez le suivant ({0} sur {1})", "Scegli il successivo ({0} di {1})", "次を選んでください（{0} / {1}）", "请选择下一个（{0}/{1}）", "Escolha o seguinte ({0} de {1})"],
    Measured => ["Measured: {0}", "Medido: {0}", "Gemessen: {0}", "Mesuré : {0}", "Misurato: {0}", "測定結果: {0}", "测量结果：{0}", "Medido: {0}"],
    NoMeasurements => ["No readings yet.", "Aún no hay mediciones.", "Noch keine Messungen.", "Pas encore de mesures.", "Ancora nessuna misura.", "まだ測定結果はありません。", "尚无测量。", "Ainda não há medições."],
    ReadDistance => ["Distance from {0} to {1}: {2} (x {3}, y {4}, z {5})", "Distancia de {0} a {1}: {2} (x {3}, y {4}, z {5})", "Abstand von {0} zu {1}: {2} (x {3}, y {4}, z {5})", "Distance de {0} à {1} : {2} (x {3}, y {4}, z {5})", "Distanza da {0} a {1}: {2} (x {3}, y {4}, z {5})", "{0} から {1} までの距離: {2}（x {3}、y {4}、z {5}）", "{0} 到 {1} 的距离：{2}（x {3}，y {4}，z {5}）", "Distância de {0} a {1}: {2} (x {3}, y {4}, z {5})"],
    ReadAngleEdges => ["Angle between {0} and {1}: {2}", "Ángulo entre {0} y {1}: {2}", "Winkel zwischen {0} und {1}: {2}", "Angle entre {0} et {1} : {2}", "Angolo tra {0} e {1}: {2}", "{0} と {1} の間の角度: {2}", "{0} 与 {1} 之间的角度：{2}", "Ângulo entre {0} e {1}: {2}"],
    ReadAngle => ["Angle at {1} between {0} and {2}: {3}", "Ángulo en {1} entre {0} y {2}: {3}", "Winkel bei {1} zwischen {0} und {2}: {3}", "Angle en {1} entre {0} et {2} : {3}", "Angolo in {1} tra {0} e {2}: {3}", "{1} での {0} と {2} の間の角度: {3}", "{1} 处 {0} 与 {2} 之间的角度：{3}", "Ângulo em {1} entre {0} e {2}: {3}"],
    RefHole => ["hole {0}", "agujero {0}", "Loch {0}", "trou {0}", "foro {0}", "穴 {0}", "孔 {0}", "furo {0}"],
    RefPost => ["post {0}", "poste {0}", "Säule {0}", "plot {0}", "colonnina {0}", "ポスト {0}", "立柱 {0}", "poste {0}"],
    RefOuterCorner => ["outer corner ({0})", "esquina exterior ({0})", "Außenecke ({0})", "coin extérieur ({0})", "angolo esterno ({0})", "外側の角 ({0})", "外角 ({0})", "canto exterior ({0})"],
    RefInnerCorner => ["inner corner ({0})", "esquina interior ({0})", "Innenecke ({0})", "coin intérieur ({0})", "angolo interno ({0})", "内側の角 ({0})", "内角 ({0})", "canto interior ({0})"],
    RefOuterEdge => ["outer edge ({0}) to ({1})", "arista exterior ({0}) a ({1})", "Außenkante ({0}) bis ({1})", "arête extérieure ({0}) à ({1})", "spigolo esterno ({0}) a ({1})", "外側の辺 ({0}) から ({1})", "外棱 ({0}) 至 ({1})", "aresta exterior ({0}) a ({1})"],
    RefInnerEdge => ["inner edge ({0}) to ({1})", "arista interior ({0}) a ({1})", "Innenkante ({0}) bis ({1})", "arête intérieure ({0}) à ({1})", "spigolo interno ({0}) a ({1})", "内側の辺 ({0}) から ({1})", "内棱 ({0}) 至 ({1})", "aresta interior ({0}) a ({1})"],
    ExportMenu => ["Export", "Exportar", "Exportieren", "Exporter", "Esporta", "エクスポート", "导出", "Exportar"],
    ExportScript => ["Script (.scad)", "Script (.scad)", "Skript (.scad)", "Script (.scad)", "Script (.scad)", "スクリプト (.scad)", "脚本 (.scad)", "Script (.scad)"],
    ExportStep => ["STEP file (.step)", "Fichero STEP (.step)", "STEP-Datei (.step)", "Fichier STEP (.step)", "File STEP (.step)", "STEP ファイル (.step)", "STEP 文件 (.step)", "Ficheiro STEP (.step)"],
    ExportReport => ["Fit report (.txt)", "Informe de ajuste (.txt)", "Passungsbericht (.txt)", "Rapport d'ajustement (.txt)", "Rapporto di adattamento (.txt)", "適合レポート (.txt)", "适配报告 (.txt)", "Relatório de ajuste (.txt)"],
    ExportTitle => ["Export as", "Exportar como", "Exportieren als", "Exporter sous", "Esporta come", "名前を付けてエクスポート", "导出为", "Exportar como"],
    ExportOverwrite => ["Write over the file if it exists", "Sobrescribir el fichero si existe", "Datei überschreiben, falls vorhanden", "Écraser le fichier s'il existe", "Sovrascrivi il file se esiste", "ファイルが存在する場合は上書きする", "若文件已存在则覆盖", "Substituir o ficheiro se existir"],
    Exported => ["Exported {0} ({1} bytes)", "Exportado {0} ({1} bytes)", "{0} exportiert ({1} Byte)", "{0} exporté ({1} octets)", "Esportato {0} ({1} byte)", "{0} をエクスポートしました（{1} バイト）", "已导出 {0}（{1} 字节）", "Exportado {0} ({1} bytes)"],
    ReportShow => ["Fit report...", "Informe de ajuste...", "Passungsbericht...", "Rapport d'ajustement...", "Rapporto di adattamento...", "適合レポート...", "适配报告...", "Relatório de ajuste..."],
    ReportTitle => ["Fit report", "Informe de ajuste", "Passungsbericht", "Rapport d'ajustement", "Rapporto di adattamento", "適合レポート", "适配报告", "Relatório de ajuste"],
    ReportCopy => ["Copy the report", "Copiar el informe", "Bericht kopieren", "Copier le rapport", "Copia il rapporto", "レポートをコピー", "复制报告", "Copiar o relatório"],
    ReportCopied => ["Report copied to the clipboard", "Informe copiado al portapapeles", "Bericht in die Zwischenablage kopiert", "Rapport copié dans le presse-papiers", "Rapporto copiato negli appunti", "レポートをクリップボードにコピーしました", "报告已复制到剪贴板", "Relatório copiado para a área de transferência"],
}

impl Msg {
    /// The text in a language.
    pub fn tr(self, lang: Lang) -> &'static str {
        self.words()[lang as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sentence_has_a_text_in_each_of_the_eight_languages() {
        for m in Msg::all() {
            for l in Lang::ALL {
                assert!(!m.tr(l).trim().is_empty(), "{m:?} in {}", l.code());
            }
        }
    }

    #[test]
    fn the_list_of_sentences_has_no_repeats() {
        let all = Msg::all();
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "{a:?} is listed twice");
        }
    }

    #[test]
    fn a_template_has_the_same_places_in_every_language() {
        for m in Msg::all() {
            let places = |l: Lang| {
                (0..8)
                    .filter(|k| m.tr(l).contains(&format!("{{{k}}}")))
                    .count()
            };
            for l in Lang::ALL {
                assert_eq!(places(l), places(Lang::En), "{m:?} in {}", l.code());
            }
        }
    }

    #[test]
    fn the_order_of_the_languages_matches_the_lists() {
        for (k, l) in Lang::ALL.into_iter().enumerate() {
            assert_eq!(l as usize, k);
        }
        assert_eq!(Msg::Cancel.tr(Lang::Es), "Cancelar");
        assert_eq!(Msg::Cancel.tr(Lang::De), "Abbrechen");
        assert_eq!(Msg::Save.tr(Lang::Pt), "Guardar");
        assert_eq!(Msg::Undo.tr(Lang::Pt), "Desfazer");
    }

    #[test]
    fn the_codes_of_the_languages_are_those_of_the_diagnostics() {
        for (l, code) in Lang::ALL.into_iter().zip(crate::messages::LANGUAGES) {
            assert_eq!(l.code(), code);
        }
    }

    #[test]
    fn a_language_is_found_from_the_code_of_the_system() {
        assert_eq!(Lang::from_code("es_ES.UTF-8"), Some(Lang::Es));
        assert_eq!(Lang::from_code("PT-pt"), Some(Lang::Pt));
        assert_eq!(Lang::from_code("ja"), Some(Lang::Ja));
        assert_eq!(Lang::from_code("xx"), None);
        assert_eq!(Lang::from_code(""), None);
    }

    #[test]
    fn the_places_of_a_template_are_filled() {
        assert_eq!(fill("{1} and {0}", &["a", "b"]), "b and a");
        assert_eq!(fill("none", &["a"]), "none");
    }
}
