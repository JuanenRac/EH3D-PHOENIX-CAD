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
