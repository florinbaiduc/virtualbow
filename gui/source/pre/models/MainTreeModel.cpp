#include "MainTreeModel.hpp"
#include "pre/utils/ListUtils.hpp"
#include <QModelIndex>
#include <QIcon>
#include <algorithm>

MainTreeModel::MainTreeModel():
    bow(nullptr)
{
    QObject::connect(this, &QAbstractItemModel::dataChanged, this, &MainTreeModel::contentModified);
    QObject::connect(this, &QAbstractItemModel::rowsInserted, this, &MainTreeModel::contentModified);
    QObject::connect(this, &QAbstractItemModel::rowsRemoved, this, &MainTreeModel::contentModified);
    QObject::connect(this, &QAbstractItemModel::rowsMoved, this, &MainTreeModel::contentModified);
}

void MainTreeModel::setBowModel(BowModel* bow) {
    beginResetModel();
    this->bow = bow;
    endResetModel();
}

// ----- Helpers -----

int MainTreeModel::parentRowFor(int itemType) {
    switch(itemType) {
        case ItemType::MATERIAL:      return TopLevelItem::MATERIALS;
        case ItemType::LAYER_UPPER:   return TopLevelItem::LAYERS_UPPER;
        case ItemType::LAYER_LOWER:   return TopLevelItem::LAYERS_LOWER;
        case ItemType::SEGMENT_UPPER: return TopLevelItem::PROFILE_UPPER;
        case ItemType::SEGMENT_LOWER: return TopLevelItem::PROFILE_LOWER;
    }
    return -1;
}

LimbSide MainTreeModel::sideForLayerType(int itemType) {
    return itemType == ItemType::LAYER_LOWER ? LimbSide::Lower : LimbSide::Upper;
}

LimbSide MainTreeModel::sideForSegmentType(int itemType) {
    return itemType == ItemType::SEGMENT_LOWER ? LimbSide::Lower : LimbSide::Upper;
}

LimbSide MainTreeModel::sideForTopLevel(int row) {
    switch(row) {
        case TopLevelItem::LAYERS_LOWER:
        case TopLevelItem::PROFILE_LOWER:
        case TopLevelItem::WIDTH_LOWER:
            return LimbSide::Lower;
        default:
            return LimbSide::Upper;
    }
}

LimbSection& MainTreeModel::sectionFor(LimbSide side) {
    return side == LimbSide::Upper ? bow->section.upper : bow->section.lower;
}

std::list<ProfileSegment>& MainTreeModel::profileFor(LimbSide side) {
    return side == LimbSide::Upper ? bow->profile.upper : bow->profile.lower;
}

// ----- Materials (shared) -----

bool MainTreeModel::canInsertMaterial(const QModelIndexList& indexes) {
    if(indexes.size() == 1) {
        if(indexes[0].internalId() == ItemType::MATERIAL) {
            return true;
        }
        if(indexes[0].internalId() == ItemType::TOPLEVEL && indexes[0].row() == TopLevelItem::MATERIALS) {
            return true;
        }
    }
    return false;
}

void MainTreeModel::insertMaterial(int row) {
    if(row < 0 || row > (int)bow->section.materials.size()) {
        throw std::invalid_argument("Invalid material index for insertion");
    }

    Material material {
        .name = bow->generateMaterialName(),
        .color = "#d0b391",
        .density = 675.0,
        .youngs_modulus = 12e9,
        .shear_modulus = 6e9
    };

    QModelIndex parent = createIndex(TopLevelItem::MATERIALS, 0, ItemType::TOPLEVEL);
    beginInsertRows(parent, row, row);

    auto position = std::next(bow->section.materials.begin(), row);
    bow->section.materials.insert(position, material);

    endInsertRows();
}

void MainTreeModel::appendMaterial() {
    insertMaterial((int)bow->section.materials.size());
}

void MainTreeModel::removeMaterial(int row) {
    if(row < 0 || row >= (int)bow->section.materials.size()) {
        throw std::invalid_argument("Invalid material index for removal");
    }

    QModelIndex parent = createIndex(TopLevelItem::MATERIALS, 0, ItemType::TOPLEVEL);
    beginRemoveRows(parent, row, row);
    auto position = std::next(bow->section.materials.begin(), row);
    bow->section.materials.erase(position);
    endRemoveRows();
}

void MainTreeModel::swapMaterials(int i, int j) {
    if(i == j || i < 0 || j < 0 || i >= (int)bow->section.materials.size() || j >= (int)bow->section.materials.size()) {
        throw std::invalid_argument("Invalid material indices for swapping");
    }
    if(i > j) std::swap(i, j);

    QModelIndex parent = createIndex(TopLevelItem::MATERIALS, 0, ItemType::TOPLEVEL);
    beginMoveRows(parent, i, i, parent, j);
    beginMoveRows(parent, j, j, parent, i);
    swapListNodes(bow->section.materials, i, j);
    endMoveRows();
}

// ----- Layers (per-limb) -----

bool MainTreeModel::canInsertLayer(const QModelIndexList& indexes) {
    if(indexes.size() != 1) return false;
    int id = indexes[0].internalId();
    if(id == ItemType::LAYER_UPPER || id == ItemType::LAYER_LOWER) return true;
    if(id == ItemType::TOPLEVEL && (indexes[0].row() == TopLevelItem::LAYERS_UPPER ||
                                    indexes[0].row() == TopLevelItem::LAYERS_LOWER)) {
        return true;
    }
    return false;
}

void MainTreeModel::insertLayer(LimbSide side, int row) {
    auto& layers = layersFor(side);
    if(row < 0 || row > (int)layers.size()) {
        throw std::invalid_argument("Invalid layer index for insertion");
    }

    Layer layer {
        .name = bow->generateLayerName(),
        .material = bow->section.materials.empty() ? "" : bow->section.materials.front().name,
        .height = {{0.0, 0.01}, {1.0, 0.01}}
    };

    int parentRow = (side == LimbSide::Upper) ? TopLevelItem::LAYERS_UPPER : TopLevelItem::LAYERS_LOWER;
    QModelIndex parent = createIndex(parentRow, 0, ItemType::TOPLEVEL);
    beginInsertRows(parent, row, row);
    auto position = std::next(layers.begin(), row);
    layers.insert(position, layer);
    endInsertRows();
}

void MainTreeModel::appendLayer(LimbSide side) {
    insertLayer(side, (int)layersFor(side).size());
}

void MainTreeModel::removeLayer(LimbSide side, int row) {
    auto& layers = layersFor(side);
    if(row < 0 || row >= (int)layers.size()) {
        throw std::invalid_argument("Invalid layer index for removal");
    }
    int parentRow = (side == LimbSide::Upper) ? TopLevelItem::LAYERS_UPPER : TopLevelItem::LAYERS_LOWER;
    QModelIndex parent = createIndex(parentRow, 0, ItemType::TOPLEVEL);
    beginRemoveRows(parent, row, row);
    auto position = std::next(layers.begin(), row);
    layers.erase(position);
    endRemoveRows();
}

void MainTreeModel::swapLayers(LimbSide side, int i, int j) {
    auto& layers = layersFor(side);
    if(i == j || i < 0 || j < 0 || i >= (int)layers.size() || j >= (int)layers.size()) {
        throw std::invalid_argument("Invalid layer indices for swapping");
    }
    int parentRow = (side == LimbSide::Upper) ? TopLevelItem::LAYERS_UPPER : TopLevelItem::LAYERS_LOWER;
    QModelIndex parent = createIndex(parentRow, 0, ItemType::TOPLEVEL);
    beginMoveRows(parent, i, i, parent, j);
    beginMoveRows(parent, j, j, parent, i);
    swapListNodes(layers, i, j);
    endMoveRows();
}

// ----- Profile segments (per-limb) -----

bool MainTreeModel::canInsertSegment(const QModelIndexList& indexes) {
    if(indexes.size() != 1) return false;
    int id = indexes[0].internalId();
    if(id == ItemType::SEGMENT_UPPER || id == ItemType::SEGMENT_LOWER) return true;
    if(id == ItemType::TOPLEVEL && (indexes[0].row() == TopLevelItem::PROFILE_UPPER ||
                                    indexes[0].row() == TopLevelItem::PROFILE_LOWER)) {
        return true;
    }
    return false;
}

void MainTreeModel::insertSegment(LimbSide side, int row, SegmentType type) {
    auto& segments = profileFor(side);
    if(row < 0 || row > (int)segments.size()) {
        throw std::invalid_argument("Invalid segment index for insertion");
    }
    int parentRow = (side == LimbSide::Upper) ? TopLevelItem::PROFILE_UPPER : TopLevelItem::PROFILE_LOWER;
    QModelIndex parent = createIndex(parentRow, 0, ItemType::TOPLEVEL);
    beginInsertRows(parent, row, row);
    auto position = std::next(segments.begin(), row);
    segments.insert(position, createDefaultSegment(type));
    endInsertRows();
}

void MainTreeModel::appendSegment(LimbSide side, SegmentType type) {
    insertSegment(side, (int)profileFor(side).size(), type);
}

void MainTreeModel::removeSegment(LimbSide side, int row) {
    auto& segments = profileFor(side);
    if(row < 0 || row >= (int)segments.size()) {
        throw std::invalid_argument("Invalid segment index for removal");
    }
    int parentRow = (side == LimbSide::Upper) ? TopLevelItem::PROFILE_UPPER : TopLevelItem::PROFILE_LOWER;
    QModelIndex parent = createIndex(parentRow, 0, ItemType::TOPLEVEL);
    beginRemoveRows(parent, row, row);
    auto position = std::next(segments.begin(), row);
    segments.erase(position);
    endRemoveRows();
}

void MainTreeModel::swapSegments(LimbSide side, int i, int j) {
    auto& segments = profileFor(side);
    if(i == j || i < 0 || j < 0 || i >= (int)segments.size() || j >= (int)segments.size()) {
        throw std::invalid_argument("Invalid segment indices for swapping");
    }
    int parentRow = (side == LimbSide::Upper) ? TopLevelItem::PROFILE_UPPER : TopLevelItem::PROFILE_LOWER;
    QModelIndex parent = createIndex(parentRow, 0, ItemType::TOPLEVEL);
    beginMoveRows(parent, i, i, parent, j);
    beginMoveRows(parent, j, j, parent, i);
    swapListNodes(segments, i, j);
    endMoveRows();
}

// ----- Generic remove/move -----

bool MainTreeModel::canRemoveIndexes(QModelIndexList& indexes) {
    for(QModelIndex index: indexes) {
        if(index.internalId() == ItemType::TOPLEVEL) return false;
    }
    return true;
}

void MainTreeModel::removeIndexes(QModelIndexList indexes) {
    std::sort(indexes.begin(), indexes.end(), [](const auto& lhs, const auto& rhs){
        return lhs.row() > rhs.row();
    });

    for(QModelIndex index: indexes) {
        switch(index.internalId()) {
        case ItemType::MATERIAL:      removeMaterial(index.row()); break;
        case ItemType::LAYER_UPPER:   removeLayer(LimbSide::Upper, index.row()); break;
        case ItemType::LAYER_LOWER:   removeLayer(LimbSide::Lower, index.row()); break;
        case ItemType::SEGMENT_UPPER: removeSegment(LimbSide::Upper, index.row()); break;
        case ItemType::SEGMENT_LOWER: removeSegment(LimbSide::Lower, index.row()); break;
        }
    }
}

bool MainTreeModel::canMoveIndexesUp(const QModelIndexList& indexes) {
    if(indexes.isEmpty() || indexes.front().internalId() == ItemType::TOPLEVEL) return false;
    for(int i = 1; i < indexes.size(); ++i) {
        if(indexes[i].internalId() != indexes[0].internalId()) return false;
    }
    for(QModelIndex index: indexes) {
        if(index.row() == 0) return false;
    }
    return true;
}

bool MainTreeModel::canMoveIndexesDown(const QModelIndexList& indexes) {
    if(indexes.isEmpty() || indexes.front().internalId() == ItemType::TOPLEVEL) return false;
    for(int i = 1; i < indexes.size(); ++i) {
        if(indexes[i].internalId() != indexes[0].internalId()) return false;
    }
    QModelIndex parent = indexes.first().parent();
    int lastIndex = rowCount(parent) - 1;
    for(QModelIndex index: indexes) {
        if(index.row() == lastIndex) return false;
    }
    return true;
}

void MainTreeModel::moveIndexesUp(QModelIndexList indexes) {
    std::sort(indexes.begin(), indexes.end(), [](const auto& lhs, const auto& rhs){
        return lhs.row() < rhs.row();
    });
    for(QModelIndex index: indexes) {
        switch(index.internalId()) {
        case ItemType::MATERIAL:      swapMaterials(index.row(), index.row() - 1); break;
        case ItemType::LAYER_UPPER:   swapLayers(LimbSide::Upper, index.row(), index.row() - 1); break;
        case ItemType::LAYER_LOWER:   swapLayers(LimbSide::Lower, index.row(), index.row() - 1); break;
        case ItemType::SEGMENT_UPPER: swapSegments(LimbSide::Upper, index.row(), index.row() - 1); break;
        case ItemType::SEGMENT_LOWER: swapSegments(LimbSide::Lower, index.row(), index.row() - 1); break;
        }
    }
}

void MainTreeModel::moveIndexesDown(QModelIndexList indexes) {
    std::sort(indexes.begin(), indexes.end(), [](const auto& lhs, const auto& rhs){
        return lhs.row() > rhs.row();
    });
    for(QModelIndex index: indexes) {
        switch(index.internalId()) {
        case ItemType::MATERIAL:      swapMaterials(index.row(), index.row() + 1); break;
        case ItemType::LAYER_UPPER:   swapLayers(LimbSide::Upper, index.row(), index.row() + 1); break;
        case ItemType::LAYER_LOWER:   swapLayers(LimbSide::Lower, index.row(), index.row() + 1); break;
        case ItemType::SEGMENT_UPPER: swapSegments(LimbSide::Upper, index.row(), index.row() + 1); break;
        case ItemType::SEGMENT_LOWER: swapSegments(LimbSide::Lower, index.row(), index.row() + 1); break;
        }
    }
}

// ----- QAbstractItemModel API -----

QModelIndex MainTreeModel::index(int row, int column, const QModelIndex &parent) const {
    if(column != 0) return QModelIndex();
    if(!parent.isValid()) return createIndex(row, column, ItemType::TOPLEVEL);

    switch(parent.row()) {
        case TopLevelItem::MATERIALS:    return createIndex(row, column, ItemType::MATERIAL);
        case TopLevelItem::LAYERS_UPPER: return createIndex(row, column, ItemType::LAYER_UPPER);
        case TopLevelItem::LAYERS_LOWER: return createIndex(row, column, ItemType::LAYER_LOWER);
        case TopLevelItem::PROFILE_UPPER: return createIndex(row, column, ItemType::SEGMENT_UPPER);
        case TopLevelItem::PROFILE_LOWER: return createIndex(row, column, ItemType::SEGMENT_LOWER);
    }
    return QModelIndex();
}

QModelIndex MainTreeModel::parent(const QModelIndex &index) const {
    if(!index.isValid()) return QModelIndex();
    if(index.internalId() == ItemType::TOPLEVEL) return QModelIndex();

    int parentRow = parentRowFor((int)index.internalId());
    if(parentRow >= 0) return createIndex(parentRow, 0, ItemType::TOPLEVEL);
    return QModelIndex();
}

int MainTreeModel::rowCount(const QModelIndex &parent) const {
    if(bow == nullptr) return 0;
    if(!parent.isValid()) return TopLevelItem::COUNT;

    if(parent.internalId() == ItemType::TOPLEVEL) {
        switch(parent.row()) {
            case TopLevelItem::MATERIALS:    return (int)bow->section.materials.size();
            case TopLevelItem::LAYERS_UPPER: return (int)bow->section.upper.layers.size();
            case TopLevelItem::LAYERS_LOWER: return (int)bow->section.lower.layers.size();
            case TopLevelItem::PROFILE_UPPER: return (int)bow->profile.upper.size();
            case TopLevelItem::PROFILE_LOWER: return (int)bow->profile.lower.size();
        }
    }
    return 0;
}

int MainTreeModel::columnCount(const QModelIndex &parent) const {
    return 1;
}

Qt::ItemFlags MainTreeModel::flags(const QModelIndex &index) const {
    Qt::ItemFlags flags = QAbstractItemModel::flags(index);
    int id = (int)index.internalId();
    if(id == ItemType::MATERIAL ||
       id == ItemType::LAYER_UPPER || id == ItemType::LAYER_LOWER) {
        flags |= Qt::ItemIsEditable;
    }
    return flags;
}

QVariant MainTreeModel::data(const QModelIndex &index, int role) const {
    if(bow == nullptr) return QVariant();

    if(!index.parent().isValid()) {
        switch(role) {
            case Qt::DisplayRole:    return topLevelItemName(index.row());
            case Qt::ToolTipRole:    return topLevelToolTip(index.row());
            case Qt::DecorationRole: return topLevelItemIcon(index.row());
            default: return QVariant();
        }
    }

    int id = (int)index.internalId();

    if(id == ItemType::MATERIAL) {
        auto& material = *std::next(bow->section.materials.begin(), index.row());
        switch(role) {
            case Qt::DisplayRole: case Qt::EditRole:
                return QString::fromStdString(material.name);
            case Qt::ToolTipRole:
                return "User-defined material \"" + QString::fromStdString(material.name) + "\"";
            case Qt::DecorationRole: return QIcon(":/icons/model-material.svg");
            default: return QVariant();
        }
    }

    if(id == ItemType::LAYER_UPPER || id == ItemType::LAYER_LOWER) {
        LimbSide side = sideForLayerType(id);
        auto& layers = (side == LimbSide::Upper) ? bow->section.upper.layers : bow->section.lower.layers;
        auto& layer = *std::next(layers.begin(), index.row());
        switch(role) {
            case Qt::DisplayRole: case Qt::EditRole:
                return QString::fromStdString(layer.name);
            case Qt::ToolTipRole:
                return "User-defined layer \"" + QString::fromStdString(layer.name) + "\"";
            case Qt::DecorationRole: return QIcon(":/icons/model-layer.svg");
            default: return QVariant();
        }
    }

    if(id == ItemType::SEGMENT_UPPER || id == ItemType::SEGMENT_LOWER) {
        LimbSide side = sideForSegmentType(id);
        switch(role) {
            case Qt::DisplayRole:    return QString::number(index.row()) + ": " + segmentName(side, index.row());
            case Qt::ToolTipRole:    return segmentTooltip(side, index.row());
            case Qt::DecorationRole: return segmentIcon(side, index.row());
            default: return QVariant();
        }
    }

    return QVariant();
}

bool MainTreeModel::setData(const QModelIndex &index, const QVariant &value, int role) {
    if(role != Qt::EditRole) return false;
    int id = (int)index.internalId();

    if(id == ItemType::MATERIAL) {
        std::string newName = value.toString().toStdString();
        if(!bow->isValidMaterialName(newName)) return false;

        Material& material = *std::next(bow->section.materials.begin(), index.row());
        std::string oldName = material.name;
        material.name = newName;

        // Update layers in BOTH limbs that referenced this material name
        for(auto& layer: bow->section.upper.layers) {
            if(layer.material == oldName) layer.material = newName;
        }
        for(auto& layer: bow->section.lower.layers) {
            if(layer.material == oldName) layer.material = newName;
        }

        emit dataChanged(index, index);
        return true;
    }

    if(id == ItemType::LAYER_UPPER || id == ItemType::LAYER_LOWER) {
        std::string name = value.toString().toStdString();
        if(!bow->isValidLayerName(name)) return false;

        LimbSide side = sideForLayerType(id);
        auto& layers = (side == LimbSide::Upper) ? bow->section.upper.layers : bow->section.lower.layers;
        std::next(layers.begin(), index.row())->name = name;
        emit dataChanged(index, index);
        return true;
    }

    return false;
}

QString MainTreeModel::topLevelItemName(int row) const {
    switch(row) {
        case TopLevelItem::COMMENTS:      return "Comments";
        case TopLevelItem::SETTINGS:      return "Settings";
        case TopLevelItem::DRAW:          return "Draw";
        case TopLevelItem::HANDLE:        return "Handle";
        case TopLevelItem::MATERIALS:     return "Materials";
        case TopLevelItem::LAYERS_UPPER:  return "Layers (upper)";
        case TopLevelItem::LAYERS_LOWER:  return "Layers (lower)";
        case TopLevelItem::PROFILE_UPPER: return "Profile (upper)";
        case TopLevelItem::PROFILE_LOWER: return "Profile (lower)";
        case TopLevelItem::WIDTH_UPPER:   return "Width (upper)";
        case TopLevelItem::WIDTH_LOWER:   return "Width (lower)";
        case TopLevelItem::STRING:        return "String";
        case TopLevelItem::MASSES:        return "Masses";
        case TopLevelItem::DAMPING:       return "Damping";
        default: throw std::invalid_argument("Unknown enum variant");
    }
}

QString MainTreeModel::topLevelToolTip(int row) const {
    switch(row) {
        case TopLevelItem::COMMENTS:      return "Comments about this bow";
        case TopLevelItem::SETTINGS:      return "Settings for the simulation";
        case TopLevelItem::DRAW:          return "Brace height, draw length and nock offset";
        case TopLevelItem::HANDLE:        return "Handle geometry";
        case TopLevelItem::MATERIALS:     return "Materials that can be assigned to the layers";
        case TopLevelItem::LAYERS_UPPER:  return "Layers that make up the upper limb";
        case TopLevelItem::LAYERS_LOWER:  return "Layers that make up the lower limb";
        case TopLevelItem::PROFILE_UPPER: return "Initial profile shape of the upper limb";
        case TopLevelItem::PROFILE_LOWER: return "Initial profile shape of the lower limb";
        case TopLevelItem::WIDTH_UPPER:   return "Width variation of the upper limb";
        case TopLevelItem::WIDTH_LOWER:   return "Width variation of the lower limb";
        case TopLevelItem::STRING:        return "Properties of the bowstring";
        case TopLevelItem::MASSES:        return "Masses of the arrow and other components";
        case TopLevelItem::DAMPING:       return "Damping properties of limbs and string";
        default: throw std::invalid_argument("Unknown enum variant");
    }
}

QIcon MainTreeModel::topLevelItemIcon(int row) const {
    switch(row) {
        case TopLevelItem::COMMENTS:      return QIcon(":/icons/model-comments.svg");
        case TopLevelItem::SETTINGS:      return QIcon(":/icons/model-settings.svg");
        case TopLevelItem::DRAW:          return QIcon(":/icons/model-draw.svg");
        case TopLevelItem::HANDLE:        return QIcon(":/icons/model-handle.svg");
        case TopLevelItem::MATERIALS:     return QIcon(":/icons/model-materials.svg");
        case TopLevelItem::LAYERS_UPPER:  return QIcon(":/icons/model-layers.svg");
        case TopLevelItem::LAYERS_LOWER:  return QIcon(":/icons/model-layers.svg");
        case TopLevelItem::PROFILE_UPPER: return QIcon(":/icons/model-profile.svg");
        case TopLevelItem::PROFILE_LOWER: return QIcon(":/icons/model-profile.svg");
        case TopLevelItem::WIDTH_UPPER:   return QIcon(":/icons/model-width.svg");
        case TopLevelItem::WIDTH_LOWER:   return QIcon(":/icons/model-width.svg");
        case TopLevelItem::STRING:        return QIcon(":/icons/model-string.svg");
        case TopLevelItem::MASSES:        return QIcon(":/icons/model-masses.svg");
        case TopLevelItem::DAMPING:       return QIcon(":/icons/model-damping.svg");
        default: throw std::invalid_argument("Unknown enum variant");
    }
}

QString MainTreeModel::segmentName(LimbSide side, int row) const {
    auto& segments = (side == LimbSide::Upper) ? bow->profile.upper : bow->profile.lower;
    const ProfileSegment& segment = *std::next(segments.begin(), row);

    if(std::holds_alternative<Line>(segment))   return "Line";
    if(std::holds_alternative<Arc>(segment))    return "Arc";
    if(std::holds_alternative<Spiral>(segment)) return "Spiral";
    if(std::holds_alternative<Spline>(segment)) return "Spline";
    throw std::invalid_argument("Unknown segment type");
}

QString MainTreeModel::segmentTooltip(LimbSide side, int row) const {
    auto& segments = (side == LimbSide::Upper) ? bow->profile.upper : bow->profile.lower;
    const ProfileSegment& segment = *std::next(segments.begin(), row);

    if(std::holds_alternative<Line>(segment))   return "Line segment defined by a single length";
    if(std::holds_alternative<Arc>(segment))    return "Arc segment defined by length and radius";
    if(std::holds_alternative<Spiral>(segment)) return "Spiral segment defined by length, start-radius and end-radius";
    if(std::holds_alternative<Spline>(segment)) return "Spline segment defined by a series of control points";
    throw std::invalid_argument("Unknown segment type");
}

QIcon MainTreeModel::segmentIcon(LimbSide side, int row) const {
    auto& segments = (side == LimbSide::Upper) ? bow->profile.upper : bow->profile.lower;
    const ProfileSegment& segment = *std::next(segments.begin(), row);

    if(std::holds_alternative<Line>(segment))   return QIcon(":/icons/segment-line.svg");
    if(std::holds_alternative<Arc>(segment))    return QIcon(":/icons/segment-arc.svg");
    if(std::holds_alternative<Spiral>(segment)) return QIcon(":/icons/segment-spiral.svg");
    if(std::holds_alternative<Spline>(segment)) return QIcon(":/icons/segment-spline.svg");
    throw std::invalid_argument("Unknown segment type");
}
