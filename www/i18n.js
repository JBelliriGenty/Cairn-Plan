// cairn-plan translations (same engine as cairn-nas and cairn-net). The code
// is written in English; each language maps an English source string to its
// translation (missing = English shown). Placeholders: t("{n} tickets", { n: 3 }).
// Plurals: tp(n, "{n} ticket", "{n} tickets").
"use strict";

const LANGS = { en: { name: "English", locale: "en-GB" }, fr: { name: "Français", locale: "fr-FR" } };

const DICT = {
  fr: {
    // frame
    "Language: {name}": "Langue : {name}", "Projects": "Projets", "Back to the projects": "Retour aux projets",
    // states, priorities, roles
    "Proposed": "Proposé", "To do": "À faire", "In progress": "En cours", "Waiting": "En attente", "Done": "Terminé", "Cancelled": "Annulé",
    "No priority": "Sans priorité", "Low": "Basse", "Medium": "Moyenne", "High": "Haute", "Urgent": "Urgente",
    "Reader": "Lecteur", "Contributor": "Contributeur", "Owner": "Responsable", "Administrator": "Administrateur",
    // projects
    "No project yet. Create the first one below.": "Aucun projet pour l'instant. Créez le premier ci-dessous.",
    "You are not a member of any project yet. Ask a project owner to add you.":
      "Vous n'êtes membre d'aucun projet. Demandez à un responsable de projet de vous ajouter.",
    "{n} open ticket": "{n} ticket ouvert", "{n} open tickets": "{n} tickets ouverts", "{n} in total": "{n} au total",
    "New project": "Nouveau projet", "Key": "Clé", "Name": "Nom", "Description": "Description",
    "Short code in capitals, used in ticket numbers (SHOP-12).": "Code court en majuscules, repris dans les numéros de ticket (SHOP-12).",
    "Create the project": "Créer le projet",
    "Space": "Espace", "No space": "Sans espace", "Project settings": "Réglages du projet",
    "The company or area the project is filed under (optional).": "L'entreprise ou le domaine où ranger le projet (facultatif).",
    "Only an administrator can move a project to another space.": "Seul un administrateur peut changer un projet d'espace.",
    // project
    "{n} member": "{n} membre", "{n} members": "{n} membres", "Show": "Afficher",
    "Open tickets": "Tickets ouverts", "All tickets": "Tous les tickets", "{n} ticket": "{n} ticket", "{n} tickets": "{n} tickets",
    "Ticket": "Ticket", "Title": "Titre", "State": "État", "Priority": "Priorité", "Assignee": "Assigné à", "Start": "Début", "End": "Fin",
    "No ticket here.": "Aucun ticket ici.",
    "Milestone": "Jalon", "All milestones": "Tous les jalons", "No milestone": "Sans jalon",
    "A family of tickets: a phase, a version.": "Une famille de tickets : une phase, une version.",
    "invalid milestone": "nom de jalon trop long",
    "Agent": "Agent", "The agent role that builds it (dev, doc); empty: the project lead; routine: no agent takes it, a session or the night routine does.":
      "Le rôle de l'agent qui le réalise (dev, doc) ; vide : le chef de projet ; routine : aucun agent ne le prend, une session ou la routine de nuit le réalise.",
    "Sub-tickets": "Sous-tickets", "Sub-ticket of": "Sous-ticket de", "Sub-tickets done": "Sous-tickets terminés",
    "{done} of {total} sub-tickets done": "{done} sous-tickets terminés sur {total}",
    "invalid agent": "rôle d'agent invalide (minuscules, chiffres, . _ -)",
    "the parent must be a ticket of the same project": "le parent doit être un ticket du même projet",
    "a sub-ticket cannot have sub-tickets": "un sous-ticket ne peut pas avoir de sous-tickets",
    "the parent is closed": "le parent est fermé",
    "this ticket has as many sub-tickets as it may": "ce ticket a déjà le nombre maximal de sous-tickets",
    "List": "Liste", "Board": "Tableau", "Search a ticket": "Rechercher un ticket", "Everyone": "Tout le monde",
    "Unassigned": "Non assigné", "All priorities": "Toutes les priorités",
    "Drag a card to another column to change its state. Only a project owner approves a proposal.":
      "Glissez une carte dans une autre colonne pour changer son état. Seul un responsable du projet valide une proposition.", "New ticket": "Nouveau ticket", "Title of the ticket": "Titre du ticket",
    "Add the ticket": "Ajouter le ticket",
    "A new ticket starts as a proposal; a project owner approves it.": "Un nouveau ticket est une proposition ; un responsable du projet la valide.",
    // members
    "Members of {name}": "Membres de {name}", "Back to the project": "Retour au projet", "Login": "Identifiant", "Role": "Rôle",
    "Reader: reads and comments. Contributor: creates and edits tickets. Owner: approves tickets and manages members.":
      "Lecteur : lit et commente. Contributeur : crée et modifie des tickets. Responsable : valide les tickets et gère les membres.",
    "Remove": "Retirer", "Add a member": "Ajouter un membre", "Add": "Ajouter",
    "The name the person signs in with, in lower case.": "Le nom avec lequel la personne se connecte, en minuscules.",
    // ticket
    "Created by {author} on {date}": "Créé par {author} le {date}",
    "Proposal: approve it to mark it as ready to be done.": "Proposition : validez-la pour la marquer comme prête à être réalisée.",
    "This ticket waits for the approval of a project owner.": "Ce ticket attend la validation d'un responsable du projet.",
    "Approved: only a project owner can change its title and description.":
      "Validé : seul un responsable du projet peut en modifier le titre et la description.",
    "What is needed, for whom, and how we will know it is done.": "Le besoin, pour qui, et à quoi on verra que c'est réussi.",
    "Nobody": "Personne", "Save": "Enregistrer", "Saved": "Enregistré", "Approve": "Valider",
    "Links": "Liens", "No link yet.": "Aucun lien pour l'instant.", "Address (https://…)": "Adresse (https://…)", "Label": "Libellé",
    "Add the link": "Ajouter le lien",
    "Comments": "Commentaires", "No comment yet.": "Aucun commentaire pour l'instant.", "Write a comment": "Écrire un commentaire", "Send": "Envoyer",
    "History": "Historique", "created the ticket": "a créé le ticket", "changed the description": "a modifié la description",
    "added the link {url}": "a ajouté le lien {url}", "removed the link {url}": "a retiré le lien {url}",
    "{field}: {old} → {new}": "{field} : {old} → {new}", "empty": "vide",
    // settings
    "Settings": "Réglages", "How notifications leave the page. What is saved here wins over the configuration file.":
      "Comment les notifications sortent de l'outil. Ce qui est enregistré ici l'emporte sur le fichier de configuration.",
    "Address": "Adresse", "Address of this page": "Adresse de cette page", "Used for the link in a notification.": "Sert au lien dans une notification.",
    "E-mail": "Courriel", "SMTP server": "Serveur SMTP", "Empty: no e-mail is sent.": "Vide : aucun courriel n'est envoyé.",
    "Port": "Port", "465: TLS from the start. Any other: STARTTLS.": "465 : TLS dès la connexion. Tout autre port : STARTTLS.",
    "User": "Utilisateur", "Password": "Mot de passe", "Leave empty to keep the one that is saved.": "Laisser vide pour garder celui qui est enregistré.",
    "Sender": "Expéditeur", "An address your SMTP server accepts.": "Une adresse que votre serveur SMTP accepte.",
    "Phone (ntfy)": "Téléphone (ntfy)", "ntfy server": "Serveur ntfy", "Empty: no push is sent.": "Vide : aucune notification sur le téléphone.",
    "Access token": "Jeton d'accès", "Topic prefix": "Préfixe des topics",
    "Each member chooses a topic that starts with it. Give the token write access to these topics only.":
      "Chaque membre choisit un topic qui commence ainsi. Ne donnez au jeton le droit d'écrire que sur ces topics.",
    "Unchanged": "Inchangé", "Try it": "Essayer", "Send a test": "Envoyer un essai",
    "Sends one message to you with the settings that are saved. The answer of the server is shown if it fails.":
      "Vous envoie un message avec les réglages enregistrés. En cas d'échec, la réponse du serveur est affichée.",
    "Sent to you. Check that it arrives.": "Envoyé. Vérifiez qu'il arrive.",
    "Agent": "Agent", "Webhook address": "Adresse du webhook", "Empty: nothing is called.": "Vide : rien n'est appelé.", "Secret": "Secret",
    "Called as soon as a ticket is approved, and when whoever approved a waiting ticket answers it: an agent can then take the ticket at once.":
      "Appelé dès qu'un ticket est validé, et quand la personne qui a validé un ticket en attente y répond : un agent peut alors prendre le ticket tout de suite.",
    "16 characters or more, the same as in the agent. It signs each call. Leave empty to keep the one that is saved.":
      "16 caractères ou plus, le même que dans l'agent. Il signe chaque appel. Laisser vide pour garder celui qui est enregistré.",
    "The agent answered.": "L'agent a répondu.",
    "invalid agent_hook_url": "adresse du webhook invalide (http://… ou https://…)", "invalid agent_hook_secret": "secret trop court : 16 caractères ou plus",
    "no agent webhook is set": "aucun webhook d'agent n'est réglé", "no secret is set for the agent webhook": "aucun secret n'est réglé pour le webhook de l'agent",
    "On my phone (ntfy)": "Sur mon téléphone (ntfy)", "ntfy topic": "Topic ntfy",
    "Choose a topic that starts with {prefix} and subscribe to it in the ntfy app. Empty: no push.":
      "Choisissez un topic qui commence par {prefix} et abonnez-vous-y dans l'application ntfy. Vide : rien sur le téléphone.",
    "Choose a topic and subscribe to it in the ntfy app. Empty: no push.":
      "Choisissez un topic et abonnez-vous-y dans l'application ntfy. Vide : rien sur le téléphone.",
    "only an administrator can change the settings": "seul un administrateur peut modifier les réglages",
    "invalid topic": "topic invalide : lettres, chiffres, - et _, avec le préfixe demandé",
    "invalid public_url": "adresse de la page invalide (https://…)", "invalid ntfy_url": "adresse du serveur ntfy invalide (https://…)",
    "invalid smtp_port": "port invalide", "invalid ntfy_prefix": "préfixe invalide : lettres, chiffres, - et _",
    "no SMTP server is set": "aucun serveur SMTP n'est réglé", "no ntfy server is set": "aucun serveur ntfy n'est réglé",
    "the proxy gave no e-mail address for you": "le proxy n'a donné aucune adresse de courriel pour vous",
    "choose your ntfy topic on the Notifications page first": "choisissez d'abord votre topic ntfy sur la page Notifications",
    // time
    "Time": "Temps", "Time spent: {t}": "Temps passé : {t}", "{label}: {value}": "{label} : {value}",
    "Time (1h30, 45m)": "Durée (1h30, 45m)", "Time spent": "Temps passé", "Day": "Jour", "Note (optional)": "Note (facultative)",
    "Log time": "Saisir le temps", "{m} min": "{m} min", "{h} h": "{h} h", "{h} h {m}": "{h} h {m}", "{d} d {h} h": "{d} j {h} h",
    "logged {t}": "a saisi {t}", "removed {t} of logged time": "a retiré {t} de temps saisi",
    "invalid time": "durée invalide : par exemple 1h30, 45m ou 90", "invalid day": "jour invalide", "no such entry": "saisie introuvable",
    "only a project owner can remove the time of someone else": "seul un responsable du projet peut retirer le temps saisi par quelqu'un d'autre",
    // files
    "Files": "Pièces jointes", "No file yet.": "Aucune pièce jointe pour l'instant.", "Attach": "Joindre",
    "Up to {n} MB per file.": "Jusqu'à {n} Mo par fichier.", "{n} kB": "{n} ko", "{n} MB": "{n} Mo",
    "attached the file {name}": "a joint le fichier {name}", "removed the file {name}": "a retiré le fichier {name}",
    "file too large": "fichier trop lourd", "empty file": "fichier vide", "invalid file name": "nom de fichier invalide",
    "no such file": "pièce jointe introuvable", "the file could not be saved": "le fichier n'a pas pu être enregistré",
    // profile photo
    "My profile": "Mon profil", "Choose a photo": "Choisir une photo", "Change the photo": "Changer la photo", "Remove the photo": "Retirer la photo",
    "Your photo shows next to your name: tickets, comments, history, Time page.":
      "Votre photo paraît à côté de votre nom : tickets, commentaires, historique, page Temps.",
    "PNG, JPEG or WebP. You frame the photo before it is saved.": "PNG, JPEG ou WebP. Vous cadrez la photo avant l'enregistrement.",
    "Frame the photo": "Cadrer la photo", "Drag the photo to place it. The slider zooms in.": "Glissez la photo pour la placer. Le curseur l'agrandit.",
    "Photo: drag it or use the arrow keys to place it": "Photo : glissez-la ou utilisez les flèches pour la placer",
    "Zoom": "Zoom", "Cancel": "Annuler",
    "An administrator can remove the photo of someone. Their initials come back.":
      "Un administrateur peut retirer la photo d'une personne. Ses initiales reviennent.",
    "An agent never opens this page: an administrator chooses its photo. It stays a square.":
      "Un agent n'ouvre jamais cette page : un administrateur choisit sa photo. Elle reste carrée.",
    "not an image (PNG, JPEG or WebP)": "ce n'est pas une image (PNG, JPEG ou WebP)", "image too large": "image trop lourde",
    "no such photo": "photo introuvable", "everyone chooses their own photo": "chacun choisit sa propre photo",
    "only an administrator can set the photo of an agent": "seul un administrateur règle la photo d'un agent",
    // notifications
    "Notifications": "Notifications", "Mark all as read": "Tout marquer comme lu", "Nothing yet.": "Rien pour l'instant.",
    "What others do on the tickets you created, are assigned to or commented.":
      "Ce que font les autres sur les tickets que vous avez créés, qui vous sont assignés ou que vous avez commentés.",
    "{actor} commented": "{actor} a commenté", "{actor} moved the ticket to {state}": "{actor} a passé le ticket à « {state} »",
    "{actor} assigned the ticket to you": "{actor} vous a assigné le ticket",
    "Also send them to me by e-mail ({email})": "Me les envoyer aussi par courriel ({email})",
    "Also send them to me by e-mail": "Me les envoyer aussi par courriel",
    // refusals (server messages)
    "request failed": "la demande a échoué",
    "Your session has ended. Your text is kept: sign in again, then send again.": "Votre session est terminée. Votre texte est gardé : reconnectez-vous, puis envoyez de nouveau.",
    "Your session has ended. What you typed is kept in this browser.": "Votre session est terminée. Ce que vous avez tapé est gardé dans ce navigateur.",
    "Sign in again (new tab)": "Se reconnecter (nouvel onglet)", "then come back to this tab and send again.": "puis revenez dans cet onglet et envoyez de nouveau.",
    "Draft restored": "Brouillon restauré", "invalid request": "demande invalide", "not found": "introuvable",
    "database error": "erreur de la base de données", "access denied": "accès refusé",
    "no such project": "projet introuvable", "no such ticket": "ticket introuvable", "no such link": "lien introuvable",
    "only an administrator can create a project": "seul un administrateur peut créer un projet",
    "invalid space": "nom d'espace trop long",
    "only a project owner can change the project": "seul un responsable du projet peut modifier le projet",
    "only an administrator can move a project to another space": "seul un administrateur peut changer un projet d'espace",
    "invalid key": "clé invalide : 2 à 10 majuscules ou chiffres, une lettre en premier", "invalid name": "nom invalide",
    "this key is already used": "cette clé est déjà prise",
    "only a project owner can manage members": "seul un responsable du projet peut gérer les membres",
    "invalid login": "identifiant invalide", "invalid role": "rôle invalide",
    "readers cannot create tickets": "un lecteur ne peut pas créer de ticket",
    "readers cannot edit tickets": "un lecteur ne peut pas modifier un ticket",
    "only a project owner can approve a ticket": "seul un responsable du projet peut valider un ticket",
    "only a project owner can change the text of an approved ticket":
      "seul un responsable du projet peut modifier le texte d'un ticket validé",
    "invalid comment": "commentaire invalide", "invalid link": "lien invalide : une adresse en http:// ou https://",
    "invalid title": "titre invalide", "invalid description": "description trop longue", "invalid state": "état invalide",
    "invalid priority": "priorité invalide", "invalid assignee": "la personne assignée doit être membre du projet",
    "invalid start_date": "date de début invalide", "invalid end_date": "date de fin invalide",
    // questions asked to an owner, and what waits on me
    "a question goes to a project owner: assign it to one": "une question va à un responsable du projet : assignez-la à l'un d'eux",
    "For me": "À moi",
    "What waits on you, in every space: the tickets assigned to you and the questions on tickets you approved.":
      "Ce qui vous attend, dans tous les espaces : les tickets qui vous sont assignés et les questions sur les tickets que vous avez validés.",
    "for less than an hour": "depuis moins d'une heure", "for {n} hour": "depuis {n} heure", "for {n} hours": "depuis {n} heures",
    "for {n} day": "depuis {n} jour", "for {n} days": "depuis {n} jours", "{author}: {text}": "{author} : {text}",
    "Update of {date}": "Mise à jour du {date}", "Send the answer": "Envoyer la réponse",
    "Your answer is a comment: whoever asked takes the ticket again.": "Votre réponse est un commentaire : la personne qui a demandé reprend le ticket.",
    "and {n} earlier notice": "et {n} notification plus ancienne", "and {n} earlier notices": "et {n} notifications plus anciennes",
    "Search": "Recherche", "A word, a ticket id, a sentence": "Un mot, un identifiant de ticket, une phrase",
    "Type two characters or more.": "Saisissez deux caractères ou plus.", "No ticket holds this text.": "Aucun ticket ne contient ce texte.",
    "The 50 newest are shown: add a word.": "Les 50 plus récents sont affichés : ajoutez un mot.",
    "In the tickets of every space you see: id, title, description and comments.":
      "Dans les tickets de tous vos espaces : identifiant, titre, description et commentaires.",
    "search for 2 to 100 characters": "cherchez 2 à 100 caractères",
    "Edit the description": "Modifier la description", "Documentation": "Documentation",
    "Wake up": "Réveiller le",
    "Until that day the ticket is out of « For me » and no agent takes it.": "Jusqu'à ce jour, le ticket est hors de « À moi » et aucun agent ne le prend.",
    "This ticket sleeps until {date}.": "Ce ticket dort jusqu'au {date}.",
    "This ticket is back: its wake-up date has come": "Ce ticket est de retour : sa date de réveil est arrivée",
    "this ticket sleeps until its wake-up date": "ce ticket dort jusqu'à sa date de réveil", "invalid wake_date": "date de réveil invalide",
    "What the agent is doing": "Ce que fait l'agent",
    "What the agent that takes approved tickets is doing today.": "Ce que fait aujourd'hui l'agent qui prend les tickets validés.",
    "No agent is set: give its webhook address and secret in the settings.": "Aucun agent n'est réglé : donnez son adresse de webhook et son secret dans les réglages.",
    "The agent does not answer: {error}": "L'agent ne répond pas : {error}",
    "The agent works on": "L'agent travaille sur", "The agent has no ticket in hand.": "L'agent n'a aucun ticket en main.",
    "{n} ticket waits for the agent.": "{n} ticket attend l'agent.", "{n} tickets wait for the agent.": "{n} tickets attendent l'agent.",
    "Stopped until the next window: {roles}.": "Arrêté jusqu'à la prochaine plage : {roles}.", "night": "nuit", "Today": "Aujourd'hui", "Last lines": "Dernières lignes",
    "only an administrator can see the agent": "seul un administrateur voit l'agent",
    "Create and approve": "Créer et valider", "Export": "Exporter", "Add as a proposal": "Ajouter comme proposition",
    "Create and approve: the ticket is ready to be done at once. A proposal waits for your approval.":
      "Créer et valider : le ticket est prêt à être fait tout de suite. Une proposition attend votre validation.",
    "Waiting for your answer": "En attente de votre réponse", "Nothing waits for you.": "Rien ne vous attend.",
    "Also assigned to you": "Vous sont aussi assignés", "by {author}": "de {author}",
    "{author} asks you": "{author} vous demande", "Your answer (optional)": "Votre réponse (facultatif)", "Your answer": "Votre réponse",
    "Done, close it": "Fait, fermer", "Answered: carry on": "Répondu : continuer", "Only send the answer": "Envoyer la réponse seulement",
    "Done: nothing more to do. Carry on: you approve what the ticket says comes next, with your answer.":
      "Fait : plus rien à faire. Continuer : vous validez la suite annoncée dans le ticket, avec votre réponse.",
    "Done: nothing more to do.": "Fait : plus rien à faire.",
    "Copy": "Copier", "This diagram could not be drawn. Source shown instead.": "Ce schéma n'a pas pu être dessiné. Source affichée à la place.", "Copied": "Copié", "Selected: copy it with Ctrl+C.": "Sélectionné : copiez avec Ctrl+C.",
    // time page
    "7 days": "7 jours", "30 days": "30 jours", "90 days": "90 jours", "All": "Tout", "Period": "Période", "View": "Vue",
    "Summary": "Synthèse", "Timesheet": "Relevé", "Journal": "Journal",
    "logged since the start": "saisies depuis le début", "logged over the last {n} days": "saisies sur les {n} derniers jours",
    "No time": "Aucun temps", "People": "Personnes", "Agents": "Agents", "Tickets done": "Tickets terminés",
    "in {t} on average": "en {t} en moyenne", "Time logged over the period": "Temps saisi sur la période",
    "Per day": "Par jour", "Per week": "Par semaine", "Per month": "Par mois", "Who": "Qui", "Which project": "Quel projet",
    "Which ticket": "Quel ticket", "No time in this period.": "Aucun temps sur cette période.", "Show all ({n})": "Tout afficher ({n})",
    "Person": "Personne", "Project": "Projet", "Total": "Total", "Cost": "Coût", "Tickets": "Tickets",
    "Hourly rates": "Taux horaires", "Currency": "Devise", "Nobody logged time yet.": "Personne n'a encore saisi de temps.",
    "only an administrator can set an hourly rate": "seul un administrateur peut régler un taux horaire",
    "invalid rate": "taux invalide", "invalid currency": "devise invalide",
    "Time · {name}": "Temps · {name}", "{n} project": "{n} projet", "{n} projects": "{n} projets",
    // money
    "What an hour of each person costs, in the currency each person chose. The page converts it and multiplies it by the time logged. Only administrators and project owners see the amounts. Agents report their own cost.":
      "Le coût d'une heure de chaque personne, dans la devise que chacun a choisie. La page la convertit et la multiplie par le temps saisi. Seuls les administrateurs et les responsables de projet voient les montants. Les agents donnent leur propre coût.",
    "Hourly rate": "Taux horaire", "My hourly rate": "Mon taux horaire",
    "What an hour of your time costs. The Time page multiplies it by the time you log. Only you, the administrators and the owners of your projects see it.":
      "Ce que coûte une heure de votre temps. La page Temps la multiplie par le temps que vous saisissez. Seuls vous, les administrateurs et les responsables de vos projets la voient.",
    "Currencies": "Devises", "Shown in": "Affichés en", "Value of one dollar": "Valeur d'un dollar",
    "Amounts are shown in this currency. Each member gives their hourly rate in their own currency; agents cost dollars.":
      "Les montants s'affichent dans cette devise. Chaque membre donne son taux horaire dans sa devise ; les agents coûtent des dollars.",
    "One currency per line: its code (EUR) and what one dollar is worth in it (0.92). Change the values when the rates move.":
      "Une devise par ligne : son code (EUR) et ce que vaut un dollar dans cette devise (0.92). Changez les valeurs quand les cours bougent.",
    "{a} an hour over {n} days": "{a} par heure sur {n} jours",
    "only an administrator can change the currencies": "seul un administrateur peut changer les devises",
    "only an administrator can set the hourly rate of someone else": "seul un administrateur peut régler le taux horaire d'une autre personne",
    "{a} (estimate)": "{a} (estimé)", "Hourly rate: not measured yet": "Taux horaire : pas encore mesuré",
    "Rates entered on {date}.": "Cours saisis le {date}.", "Rates not entered yet: these are indicative values.": "Cours pas encore saisis : ce sont des valeurs indicatives.",
    "Price of a million tokens": "Prix d'un million de jetons",
    "One model per line: its name, then the dollars for a million tokens of input, output, cache read and cache write (1 hour). A run that reports no cost is estimated with it.":
      "Un modèle par ligne : son nom, puis les dollars pour un million de jetons en entrée, en sortie, en lecture de cache et en écriture de cache (1 heure). Un run qui ne donne pas son coût est estimé avec ce tableau.",
    "Prices dated {date}, read at {source}.": "Prix du {date}, lus sur {source}.", "invalid prices": "tarifs invalides",
    // agents page
    "24 hours": "24 heures", "No agent at work": "Aucun agent au travail",
    "{n} agent at work": "{n} agent au travail", "{n} agents at work": "{n} agents au travail",
    "in the last 24 hours": "sur les dernières 24 heures", "in the last {n} days": "sur les {n} derniers jours",
    "Finished": "Terminés", "Tickets worked": "Tickets traités", "Waiting for an answer": "En attente d'une réponse",
    "No agent reported a run in this period.": "Aucun agent n'a envoyé d'exécution sur cette période.",
    "Now": "En ce moment", "Work": "Travaux", "The team": "L'équipe", "for {t}": "depuis {t}", "and {n} more": "et {n} autres",
    "No news": "Sans nouvelles", "Failed": "Échec", "Open the ticket {id}": "Ouvrir le ticket {id}",
    "{n} tokens written": "{n} jetons écrits", "{n} run": "{n} exécution", "{n} runs": "{n} exécutions",
    "median {t}": "médiane {t}", "{n} failed": "{n} en échec",
    "What the agents do, how long it takes and what it costs. Each agent reports its runs by itself.":
      "Ce que font les agents, en combien de temps et pour quel coût. Chaque agent envoie lui-même ses exécutions.",
    // team page (/team)
    "{n} s": "{n} s", "{n} min": "{n} min", "{n} h": "{n} h", "{n} d": "{n} j",
    "Working · for {t}": "Au travail · depuis {t}", "No news for {t}": "Sans nouvelles depuis {t}", "Resting": "Au repos", "Others": "Autres", "Agents that no member takes": "Agents qu'aucun membre ne prend",
    "Last run {t} ago": "Dernière exécution il y a {t}", "No run yet.": "Aucune exécution pour l'instant.",
    "No team is configured. An administrator lists the members in the \"team\" key of the configuration file.":
      "Aucune équipe n'est configurée. Un administrateur liste ses membres dans la clé « team » du fichier de configuration.",
    "Open the sheet": "Ouvrir sa fiche",
    "{n} member works for you.": "{n} membre travaille pour vous.", "{n} members work for you.": "{n} membres travaillent pour vous.",
    "<b>{a}</b> working, <b>{b}</b> waiting, <b>{c}</b> resting.": "<b>{a}</b> au travail, <b>{b}</b> en attente, <b>{c}</b> au repos.",
    "Open a member to see what it did and what it will do.": "Ouvrez un membre pour voir ce qu'il a fait et ce qu'il fera.",
    "Replay · {t}": "Rejeu · {t}", "Offline": "Hors ligne", "Live · {t}": "Direct · {t}",
    "{who} starts: {task}": "{who} commence : {task}", "{who} finished: {task}": "{who} a fini : {task}",
    "No run in the last 7 days.": "Aucune exécution ces 7 derniers jours.", "Nothing in its queue.": "Rien dans sa file.",
    "{x} per hour over 30 days": "{x} de l'heure sur 30 jours", "Close": "Fermer",
    "What it did": "Ce qu'il a fait", "Latest first, with the verdict of the check.": "Du plus récent au plus ancien, avec le verdict du contrôle.",
    "In its queue": "Dans sa file", "What it will take next, in order.": "Ce qu'il prendra ensuite, dans l'ordre.",
    "Cairn Plan does not answer. The page tries again in 10 seconds.": "Cairn Plan ne répond pas. La page réessaie dans 10 secondes.",
    "No run in the last 24 hours: nothing to replay.": "Aucune exécution ces dernières 24 heures : rien à rejouer.",
    "Replay of the last 24 hours, in one minute.": "Rejeu des dernières 24 heures, en une minute.",
    "Today: {n} run": "Aujourd'hui : {n} exécution", "Today: {n} runs": "Aujourd'hui : {n} exécutions",
    "Today: no run yet.": "Aujourd'hui : aucune exécution pour l'instant.",
    "Stop the replay": "Arrêter le rejeu", "Replay the last 24 hours": "Rejouer les dernières 24 heures",
    "Each agent reports its runs to Cairn Plan. The queue lists the open tickets assigned to it. The page reads again every 10 seconds.":
      "Chaque agent envoie ses exécutions à Cairn Plan. La file liste les tickets ouverts qui lui sont assignés. La page se relit toutes les 10 secondes.",
  },
};

let LANG = "en";

function t(s, vars) {
  let r = (DICT[LANG] && DICT[LANG][s]) || s;
  if (vars) r = r.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? vars[k] : m));
  return r;
}
const tp = (n, one, many, vars) => t(n > 1 || n === 0 && LANG === "en" ? many : one, Object.assign({ n }, vars));
const LOCALE = () => LANGS[LANG].locale;

// Order: the viewer's choice, then the server default (config "lang"), then the browser.
function pickLang(serverDefault) {
  let saved = null;
  try { saved = localStorage.getItem("cairn-lang"); } catch (e) { /* storage blocked */ }
  const browser = (navigator.languages || [navigator.language]).map(l => String(l).slice(0, 2)).find(l => LANGS[l]);
  return [saved, serverDefault, browser, "en"].find(l => l && LANGS[l]);
}

LANG = pickLang();
