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
    "Agent": "Agent", "The agent role that builds it (dev, doc); empty: the project lead.": "Le rôle de l'agent qui le réalise (dev, doc) ; vide : le chef de projet.",
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
    "Your session has ended. Reload the page to sign in again.": "Votre session est terminée. Rechargez la page pour vous reconnecter.", "invalid request": "demande invalide", "not found": "introuvable",
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
    "For me": "À moi", "The open tickets assigned to you, in every space.": "Les tickets ouverts qui vous sont assignés, dans tous les espaces.",
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
    "What an hour of each person costs. The page multiplies it by the time logged. Only administrators and project owners see the amounts. Agents report their own cost.":
      "Le coût d'une heure de chaque personne. La page le multiplie par le temps saisi. Seuls les administrateurs et les responsables de projet voient les montants. Les agents donnent leur propre coût.",
    "only an administrator can set an hourly rate": "seul un administrateur peut régler un taux horaire",
    "invalid rate": "taux invalide", "invalid currency": "devise invalide",
    "Time · {name}": "Temps · {name}", "{n} project": "{n} projet", "{n} projects": "{n} projets",
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
