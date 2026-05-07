#pragma once
#include "pre/models/PropertyListModel.hpp"

struct Masses;

class MassesModel: public PropertyListModel {
public:
    QPersistentModelIndex ARROW;
    QPersistentModelIndex LIMB_TIP_UPPER;
    QPersistentModelIndex LIMB_TIP_LOWER;
    QPersistentModelIndex STRING_NOCK;
    QPersistentModelIndex STRING_TIP_UPPER;
    QPersistentModelIndex STRING_TIP_LOWER;

    MassesModel(Masses& masses);
};
