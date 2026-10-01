# VisionMSG

VisionMSG est une application macOS native écrite en Rust. Elle lit les fichiers Outlook MSG, présente les métadonnées et le texte du message, ouvre les pièces jointes et enregistre un EML ou les pièces jointes à l'emplacement choisi.

## Développement

Rust 1.88 ou plus récent est requis. Depuis ce dossier, exécutez cargo run --release.

L'installateur PKG et l'archive macOS sont disponibles dans les [versions GitHub](https://github.com/marko87dev/VisionMSG/releases). Pour tester une version construite localement, ouvrez `dist/VisionMSG.app`. L'application est signée localement (signature ad hoc) ; le PKG n'est ni signé ni notarisé.

Glissez un fichier MSG sur la fenêtre ou utilisez « Choisir un fichier ». Les sauvegardes utilisent les dialogues macOS ; aucun export n'est créé sans choix explicite. « Ouvrir » une pièce jointe crée une copie temporaire qui est supprimée à la fermeture de VisionMSG.

Vous pouvez aussi ouvrir un `.msg` depuis le Finder avec « Ouvrir avec > VisionMSG ». Après avoir choisi VisionMSG comme application par défaut pour ce type de fichier dans « Lire les informations », un double-clic suffit. Cette association est propre à macOS et ne remplace pas automatiquement votre application par défaut actuelle.

Le menu Fichier donne accès à l'ouverture, aux exports EML, texte et HTML, à l'enregistrement des pièces jointes et à la fermeture du message. Les exports texte et HTML contiennent les métadonnées, le corps lisible et les noms des pièces jointes ; seuls les fichiers EML conservent les pièces jointes elles-mêmes. Le HTML généré ne reprend pas de contenu actif ni de ressources distantes du courriel. Le menu Navigation et la barre latérale ouvrent l'accueil, le message, l'aide et les informations sur l'application.

## Vérifications

Exécutez cargo fmt --check, cargo test, puis cargo build --release.

Le corps est affiché en texte lisible. Quand le MSG ne contient que du HTML, celui-ci est converti en texte pour l'affichage. L'EML conserve le HTML original et les pièces jointes.

Les exemples MSG dans tests/fixtures proviennent du projet msg_parser (licence MIT).

Licence de VisionMSG : GNU GPLv3 ou ultérieure, voir LICENSE.
